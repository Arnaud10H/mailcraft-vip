#!/bin/bash
# MailCraft VIP — mise à jour sur Mac, en un double-clic (lancé par « Mettre à jour MailCraft.app »).
# Récupère la dernière version (branche vip-ucs de notre Forgejo), la prépare, sauvegarde la boîte mail,
# remplace l'appli et la relance. Messages en français, pensés pour quelqu'un qui n'est pas technique.
#   vip-mac-update.sh [--force]   --force : réinstaller même si c'est déjà la dernière version
set -uo pipefail
SRC="$HOME/Applications/MailCraft-VIP/src"
DEST="$HOME/Applications/MailCraft VIP.app"
LOG="$HOME/Library/Logs/MailCraft-VIP-mise-a-jour.log"
mkdir -p "$(dirname "$LOG")"; exec > >(tee -a "$LOG") 2>&1
echo; echo "===== $(date '+%d/%m/%Y %H:%M') ====="

note() { osascript -e "display notification \"$1\" with title \"MailCraft VIP\"" >/dev/null 2>&1; }
etape() { printf '\n\033[1;34m▶ %s\033[0m\n' "$1"; }
echec() {
  osascript -e "display dialog \"$1\n\nRien n'a été cassé : ton MailCraft actuel est toujours là.\nPréviens Arnaud (le détail est dans Bibliothèque › Logs › MailCraft-VIP-mise-a-jour.log).\" with title \"MailCraft VIP — mise à jour impossible\" buttons {\"OK\"} default button 1 with icon caution" >/dev/null 2>&1
  echo "ÉCHEC : $1"; exit 1
}
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
command -v cargo >/dev/null || echec "Les outils de fabrication (Rust) ne sont pas installés : il faut relancer l'installation."
cd "$SRC" 2>/dev/null || echec "Le dossier de MailCraft est introuvable : il faut relancer l'installation."

etape "Recherche de la dernière version…"
git fetch -q origin vip-ucs || echec "Impossible de joindre le serveur des mises à jour. Vérifie la connexion internet puis réessaie."
AVANT=$(git rev-parse --short HEAD); APRES=$(git rev-parse --short origin/vip-ucs)
if [ "$AVANT" = "$APRES" ] && [ -d "$DEST" ] && [ "${1:-}" != "--force" ]; then
  CHOIX=$(osascript -e "button returned of (display dialog \"MailCraft est déjà à jour (version $AVANT).\" with title \"MailCraft VIP\" buttons {\"Réinstaller quand même\", \"Ouvrir MailCraft\"} default button 2)" 2>/dev/null)
  [ "$CHOIX" = "Réinstaller quand même" ] || { open "$DEST"; exit 0; }
fi
git reset -q --hard origin/vip-ucs

etape "Préparation de la nouvelle version ($APRES) — compte 5 à 15 minutes, laisse cette fenêtre ouverte…"
note "Mise à jour en cours (5 à 15 minutes)…"
case "$(uname -m)" in arm64) ARCH=aarch64 ;; *) ARCH=x86_64 ;; esac
rustup update stable >/dev/null 2>&1 || true
rustup target add "$ARCH-apple-darwin" >/dev/null 2>&1 || true
# Identifiant Google (connexion Gmail), s'il a été déposé sur ce Mac.
for f in "$SRC/.env" "$HOME/Nextcloud/Claude/password/mailcraft-google.env"; do [ -f "$f" ] && { set -a; . "$f"; set +a; }; done
# « dev » : l'appli ne cherche jamais de mise à jour chez UCS (ce serait une version sans nos fonctions VIP).
export MAILCRAFT_BUILD_SHA=dev
rm -rf "$SRC/target/macos-package"
bash packaging/macos/package.sh --arch "$ARCH" || true   # la DMG est facultative : seule l'appli compte
APP="$SRC/target/macos-package/MailCraft.app"
[ -x "$APP/Contents/MacOS/MailCraft" ] || echec "La préparation de la nouvelle version a échoué."

etape "Fermeture de MailCraft et sauvegarde de ta boîte mail…"
osascript -e 'tell application "MailCraft VIP" to quit' >/dev/null 2>&1; sleep 3
pgrep -xq MailCraft && { pkill -x MailCraft; sleep 2; }
BOITE="$HOME/Library/Application Support/MailCraft"; SAUV="$HOME/Library/Application Support/MailCraft-sauvegardes"
if [ -d "$BOITE" ]; then
  mkdir -p "$SAUV" && tar czf "$SAUV/boite-$(date +%Y%m%d-%H%M).tgz" -C "$(dirname "$BOITE")" "$(basename "$BOITE")"
  ls -1t "$SAUV"/boite-*.tgz 2>/dev/null | tail -n +4 | while read -r v; do rm -f "$v"; done
fi

etape "Installation…"
mkdir -p "$HOME/Applications"
[ -d "$DEST" ] && { rm -rf "$DEST.ancienne"; mv "$DEST" "$DEST.ancienne"; }
if ditto "$APP" "$DEST"; then rm -rf "$DEST.ancienne"
else [ -d "$DEST.ancienne" ] && mv "$DEST.ancienne" "$DEST"; echec "L'installation n'a pas pu se faire."; fi

note "MailCraft est à jour (version $APRES)."
etape "Terminé : MailCraft est à jour (version $APRES). Tu peux fermer cette fenêtre."
open "$DEST"
