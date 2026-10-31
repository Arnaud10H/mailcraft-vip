#!/usr/bin/env bash
# Lancé par mailcraft-vip-sync.timer (ou à la main) : synchro MailCraft VIP ↔ UCS-Care-Group,
# rapport dans Nextcloud/Claude/system-reports et notification.
REPO="$HOME/Code/mailcraft-vip"
REPORTS="${NEXTCLOUD_DIR:-$HOME/Nextcloud}/Claude/system-reports"; mkdir -p "$REPORTS"
OUT="$REPORTS/mailcraft-sync-$(date +%Y%m%d).md"
BEFORE=$(git -C "$REPO" rev-parse --short HEAD 2>/dev/null)
bash "$REPO/packaging/vip-sync-upstream.sh"; CODE=$?
LOG=$(ls -1t "${XDG_STATE_HOME:-$HOME/.local/state}/mailcraft-vip-sync/"*.log 2>/dev/null | head -1)
AFTER=$(git -C "$REPO" rev-parse --short HEAD 2>/dev/null)
case $CODE in
  0) STATUS="OK" ;; 2) STATUS="CONFLITS à résoudre (~/Code/mailcraft-vip-sync)" ;;
  3) STATUS="CONTRÔLE EN ÉCHEC (branche vip-sync-$(date +%Y%m%d) sur Forgejo)" ;; *) STATUS="ERREUR ($CODE)" ;;
esac
{
  echo "# MailCraft VIP — synchro upstream du $(date '+%d/%m/%Y %H:%M')"
  echo
  echo "- Résultat : **$STATUS**"
  echo "- vip-ucs installé : $BEFORE → $AFTER"
  echo "- Journal complet : \`$LOG\`"
  echo
  echo '```'
  grep -vE '^\s*$' "$LOG" | grep -vE '^[0-9a-f]{7,} ' | tail -40
  echo '```'
  if [ "$CODE" = 2 ] || [ "$CODE" = 3 ]; then
    echo
    echo "À faire : dans la discussion MailCraft (prompt dans ~/Nextcloud/Claude/projets/mailcraft/),"
    echo "demander de terminer la synchro (résolution dans le worktree, puis \`packaging/vip-sync-upstream.sh --continue\`)."
  fi
} > "$OUT"
if [ "$CODE" != 0 ]; then
  notify-send -u critical -a MailCraft "MailCraft VIP — synchro" "$STATUS — voir $OUT" 2>/dev/null
elif ! grep -q "up to date" "$LOG"; then
  notify-send -a MailCraft "MailCraft VIP mis à jour" "vip-ucs $BEFORE → $AFTER (relancer MailCraft pour l'utiliser)" 2>/dev/null
fi
exit 0
