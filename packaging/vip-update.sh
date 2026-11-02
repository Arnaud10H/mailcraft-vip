#!/usr/bin/env bash
# MailCraft VIP — mise à jour commune pour les postes Linux (JeanClaude, Bernard, tout poste Arch/CachyOS).
# Récupère la dernière version (branche vip-ucs de notre Forgejo), la compile, sauvegarde la boîte,
# installe l'app et la relance. Idempotent : déjà à jour → ne recompile pas (sauf --force).
#
#   packaging/vip-update.sh [--force] [--no-restart] [--check]
#     --force       recompiler et réinstaller même si la version installée est déjà la dernière
#     --no-restart  ne pas relancer MailCraft après l'installation (update-arch, cron)
#     --check       dire seulement s'il y a du nouveau (code 10 = oui, 0 = à jour) ; rien n'est changé
#
# Depuis un poste vierge (Bernard), tout en une commande :
#   bash <(curl -fsSL https://git.artisan-digital.eu/arnaud/mailcraft-vip/raw/branch/vip-ucs/packaging/vip-update.sh)
# Le dépôt est alors cloné dans ~/Code/mailcraft-vip (ou $MAILCRAFT_VIP_SRC). Rust manquant → pacman -S rust.
#
# Appel depuis update-arch (avant les paquets système) :
#   "$HOME/Code/mailcraft-vip/packaging/vip-update.sh" --no-restart
#
# Identifiant Google (connexion Gmail) : lu, s'il existe, dans $SRC/.env puis
# ~/Nextcloud/Claude/password/mailcraft-google.env (MAILCRAFT_GOOGLE_CLIENT_ID / _SECRET). Jamais dans git.
set -uo pipefail
SRC="${MAILCRAFT_VIP_SRC:-$HOME/Code/mailcraft-vip}"
REPO="${MAILCRAFT_VIP_REPO:-ssh://git@ikoula:2223/arnaud/mailcraft-vip.git}"
REPO_HTTPS="https://git.artisan-digital.eu/arnaud/mailcraft-vip.git"
BRANCH="vip-ucs"
STATE="${XDG_STATE_HOME:-$HOME/.local/state}/update-arch"
LOG="${XDG_STATE_HOME:-$HOME/.local/state}/mailcraft-vip-update.log"
FORCE=0; RESTART=1; CHECK=0
for a in "$@"; do case "$a" in --force) FORCE=1 ;; --no-restart) RESTART=0 ;; --check) CHECK=1 ;; *) echo "option inconnue : $a"; exit 1 ;; esac; done
mkdir -p "$(dirname "$LOG")" "$STATE"
exec > >(tee -a "$LOG") 2>&1
echo; echo "===== $(date '+%d/%m/%Y %H:%M') $(hostname) ====="

etape() { printf '\n\033[1;34m▶ %s\033[0m\n' "$1"; }
note() { command -v notify-send >/dev/null && notify-send "MailCraft VIP" "$1" >/dev/null 2>&1 || true; }
echec() { echo "ÉCHEC : $1"; note "Mise à jour impossible : $1"; exit 1; }

# Outils : git, Rust (cargo) ; sur Arch/CachyOS on propose pacman.
command -v git >/dev/null || echec "git n'est pas installé"
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
if ! command -v cargo >/dev/null; then
  if command -v pacman >/dev/null && [ "$CHECK" = 0 ]; then
    etape "Rust manquant : installation (sudo pacman -S --needed rust)…"
    sudo pacman -S --needed --noconfirm rust || echec "Rust n'a pas pu être installé"
  else
    echec "Rust (cargo) n'est pas installé : sudo pacman -S rust"
  fi
fi

# Dépôt : cloné s'il manque (SSH ikoula, sinon HTTPS en lecture).
if [ ! -d "$SRC/.git" ]; then
  [ "$CHECK" = 1 ] && { echo "pas de dépôt dans $SRC"; exit 10; }
  etape "Clonage de MailCraft VIP dans $SRC…"
  mkdir -p "$(dirname "$SRC")"
  git clone -q -b "$BRANCH" "$REPO" "$SRC" 2>/dev/null || git clone -q -b "$BRANCH" "$REPO_HTTPS" "$SRC" || echec "le dépôt n'a pas pu être cloné"
  cd "$SRC"; git remote get-url forgejo >/dev/null 2>&1 || git remote rename origin forgejo
fi
cd "$SRC" || echec "dossier $SRC introuvable"
git remote get-url forgejo >/dev/null 2>&1 || git remote add forgejo "$REPO"

etape "Recherche de la dernière version…"
git fetch -q forgejo "$BRANCH" || echec "impossible de joindre Forgejo (connexion, clé SSH ikoula ?)"
if [ "$(git branch --show-current)" != "$BRANCH" ] || [ -n "$(git status --porcelain --untracked-files=no)" ]; then
  echec "le dépôt n'est pas sur une branche $BRANCH propre : rien n'est touché ($(git branch --show-current))"
fi
git merge -q --ff-only "forgejo/$BRANCH" || echec "la branche locale ne peut pas avancer en fast-forward"
HEAD_SHA=$(git rev-parse HEAD); SHORT=$(git rev-parse --short HEAD)
INSTALLED=$(cat "$STATE/mailcraft-installed" 2>/dev/null || true)
if [ "$INSTALLED" = "$HEAD_SHA" ] && [ -x "$HOME/.local/bin/mailcraft" ] && [ "$FORCE" = 0 ]; then
  echo "déjà à jour ($SHORT)"; [ "$CHECK" = 1 ] && exit 0; exit 0
fi
[ "$CHECK" = 1 ] && { echo "nouvelle version disponible : $SHORT ($(git log -1 --format=%s | cut -c1-70))"; exit 10; }

etape "Compilation de $SHORT ($(git log -1 --format=%s | cut -c1-70)) — quelques minutes…"
note "Mise à jour en cours ($SHORT)…"
# Identifiant Google, s'il a été déposé sur ce poste ; « dev » : jamais de mise à jour UCS proposée par l'app.
for f in "$SRC/.env" "$HOME/Nextcloud/Claude/password/mailcraft-google.env"; do [ -f "$f" ] && { set -a; . "$f"; set +a; }; done
export MAILCRAFT_BUILD_SHA=dev
ulimit -n 65536 2>/dev/null || true
cargo build --release -p mailcraft -p mailcraft-cli || echec "la compilation a échoué (voir $LOG)"

etape "Sauvegarde de la boîte…"
BK="$HOME/Backups/mailcraft"; mkdir -p "$BK"
CFG="${XDG_CONFIG_HOME:-$HOME/.config}/mailcraft"
[ -f "$CFG/mailbox.mailcraft" ] && tar czf "$BK/mailbox-$(date +%Y%m%d-%H%M).tgz" -C "$(dirname "$CFG")" "$(basename "$CFG")"
ls -1t "$BK"/mailbox-*.tgz 2>/dev/null | tail -n +6 | xargs -r rm -f

etape "Installation…"
WAS_RUNNING=0
if pgrep -x mailcraft >/dev/null; then WAS_RUNNING=1; pkill -x mailcraft; sleep 2; pkill -9 -x mailcraft 2>/dev/null; fi
bash packaging/vip-install.sh || echec "l'installation a échoué"
echo "$HEAD_SHA" > "$STATE/mailcraft-installed"
note "MailCraft est à jour ($SHORT)."
etape "Terminé : MailCraft VIP $SHORT installé dans ~/.local/bin"
if [ "$RESTART" = 1 ] && [ "$WAS_RUNNING" = 1 ] && [ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ]; then
  (setsid "$HOME/.local/bin/mailcraft" --control 7991 >/dev/null 2>&1 &)
fi
exit 0
