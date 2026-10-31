#!/usr/bin/env bash
# Keep MailCraft VIP (branch vip-ucs on Forgejo) in step with UCS-Care-Group/mailcraft main.
#
#   packaging/vip-sync-upstream.sh [--dry-run] [--continue] [--no-install]
#     --dry-run    say what is new upstream and which files would conflict; change nothing
#     --continue   conflicts were resolved and committed in the sync worktree: run the checks, promote
#     --no-install push vip-ucs but don't rebuild/install here
#
# The merge happens in a worktree of its own (default ~/Code/mailcraft-vip-sync, branch vip-sync), so the
# main checkout — the one update-arch and the installed app come from — stays on vip-ucs, always buildable.
# Steps: fetch → merge upstream/main into vip-sync (from forgejo/vip-ucs; translation files: upstream's,
# then gathered again) → fmt, clippy -D warnings, every test → push vip-sync as vip-ucs (fast-forward only)
# → main checkout fast-forwards → mailbox backed up → packaging/vip-install.sh.
# vip-ucs is never rewritten: update-arch and the Mac pull it fast-forward only.
#
# Exit: 0 up to date or synced · 2 conflicts to resolve in the worktree · 3 a check failed (pushed as
#       vip-sync-<date>, vip-ucs untouched) · 1 any other error
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WT="${MAILCRAFT_SYNC_WORKTREE:-$HOME/Code/mailcraft-vip-sync}"
DRY=0; CONT=0; INSTALL=1
for a in "$@"; do case "$a" in --dry-run) DRY=1 ;; --continue) CONT=1 ;; --no-install) INSTALL=0 ;; *) echo "unknown option $a"; exit 1 ;; esac; done
STATE="${XDG_STATE_HOME:-$HOME/.local/state}/mailcraft-vip-sync"; mkdir -p "$STATE"
LOG="$STATE/$(date +%Y%m%d-%H%M%S).log"; exec > >(tee -a "$LOG") 2>&1
ls -1t "$STATE"/*.log 2>/dev/null | tail -n +31 | xargs -r rm -f
say() { echo "[$(date +%H:%M:%S)] $*"; }
say "log: $LOG"

cd "$ROOT"
git remote | grep -qx upstream || git remote add upstream https://github.com/UCS-Care-Group/mailcraft.git
git fetch -q upstream main && git fetch -q forgejo || { say "fetch failed"; exit 1; }
BEHIND=$(git rev-list --count forgejo/vip-ucs..upstream/main)
say "upstream/main $(git rev-parse --short upstream/main) — $BEHIND commit(s) not in vip-ucs"

if [ "$CONT" = 0 ]; then
  [ "$BEHIND" -eq 0 ] && { say "up to date"; exit 0; }
  git log --oneline --no-merges forgejo/vip-ucs..upstream/main | head -60
  if [ "$DRY" = 1 ]; then
    say "dry run: files the merge would leave in conflict"
    git merge-tree --write-tree --name-only forgejo/vip-ucs upstream/main | sed -n '2,/^$/p'
    exit 0
  fi
  if [ -d "$WT/.git" ] || [ -f "$WT/.git" ]; then
    git -C "$WT" merge --abort 2>/dev/null; git -C "$WT" reset -q --hard
    git -C "$WT" checkout -q -B vip-sync forgejo/vip-ucs
  else
    git worktree prune; git worktree add -q -B vip-sync "$WT" forgejo/vip-ucs || { say "worktree failed"; exit 1; }
  fi
  cd "$WT"
  if ! git merge --no-edit -q upstream/main; then
    # The translation files are generated: take upstream's; they are gathered again below.
    for f in $(git diff --name-only --diff-filter=U | grep '^crates/i18n/po/'); do git checkout -q --theirs -- "$f" && git add "$f"; done
    if [ -n "$(git diff --name-only --diff-filter=U)" ]; then
      say "CONFLICTS in $WT (branch vip-sync) — resolve as docs/vip-maintenance.md says, commit, then: $0 --continue"
      git diff --name-only --diff-filter=U
      exit 2
    fi
    git commit -q --no-edit
  fi
else
  cd "$WT" || { say "--continue: no worktree at $WT"; exit 1; }
  [ "$(git branch --show-current)" = vip-sync ] || { say "--continue: $WT is not on vip-sync"; exit 1; }
  [ -z "$(git status --porcelain)" ] || { say "--continue: conflicts or uncommitted changes left"; git status --short | head; exit 1; }
  git merge-base --is-ancestor upstream/main HEAD || say "note: vip-sync doesn't hold upstream/main $(git rev-parse --short upstream/main) yet"
fi

ulimit -n 65536 2>/dev/null || true   # the net tests open many sockets
FAIL=""
say "i18n template"; cargo xtask i18n >/dev/null && { git add crates/i18n/po; git diff --cached --quiet || git commit -qm "VIP: messages.pot gathered again after merging upstream"; }
say "fmt";    cargo fmt --all --check >/dev/null || FAIL=fmt
say "clippy"; [ -z "$FAIL" ] && { cargo clippy -q --workspace --all-targets -- -D warnings || FAIL=clippy; }
say "tests";  [ -z "$FAIL" ] && { cargo test -q --workspace --no-fail-fast > "$STATE/last-tests.txt" 2>&1 || FAIL=tests; }
[ -f "$STATE/last-tests.txt" ] && grep -E "^test result" "$STATE/last-tests.txt" | awk '{p+=$4; f+=$6} END {print "tests: " p " passed, " f " failed"}'
if [ -n "$FAIL" ]; then
  BR="vip-sync-$(date +%Y%m%d)"; git push -q -f forgejo "vip-sync:$BR"
  say "CHECK FAILED ($FAIL): pushed as $BR on Forgejo, vip-ucs untouched"
  grep -E "^test .* FAILED" "$STATE/last-tests.txt" 2>/dev/null | head -20
  exit 3
fi

git push -q forgejo vip-sync:vip-ucs || { say "push to vip-ucs refused (not a fast-forward?)"; exit 1; }
say "vip-ucs → $(git rev-parse --short HEAD) on Forgejo"
cd "$ROOT"; git fetch -q forgejo
if [ "$(git branch --show-current)" = vip-ucs ] && [ -z "$(git status --porcelain)" ]; then
  git merge -q --ff-only forgejo/vip-ucs || { say "main checkout can't fast-forward: left as it is"; INSTALL=0; }
else
  say "main checkout not on a clean vip-ucs: left as it is, nothing installed"; INSTALL=0
fi
if [ "$INSTALL" = 1 ]; then
  BK="$HOME/Backups/mailcraft"; mkdir -p "$BK"
  for d in "${XDG_CONFIG_HOME:-$HOME/.config}/mailcraft" "$HOME/Library/Application Support/MailCraft"; do
    [ -f "$d/mailbox.mailcraft" ] && tar czf "$BK/mailbox-$(date +%Y%m%d-%H%M).tgz" -C "$(dirname "$d")" "$(basename "$d")"
  done
  ls -1t "$BK"/mailbox-*.tgz 2>/dev/null | tail -n +6 | xargs -r rm -f
  if bash packaging/vip-install.sh; then
    mkdir -p "${XDG_STATE_HOME:-$HOME/.local/state}/update-arch"
    git rev-parse HEAD > "${XDG_STATE_HOME:-$HOME/.local/state}/update-arch/mailcraft-installed"
    say "installed $(git log -1 --format='%h %s' | cut -c1-70)"
  else
    say "install failed"; exit 1
  fi
fi
exit 0
