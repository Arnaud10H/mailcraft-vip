#!/bin/bash
# MailCraft VIP — première installation sur un Mac (à lancer une fois, par Arnaud ou son Claude).
# Installe les outils (Xcode Command Line Tools, Rust), récupère le code depuis notre Forgejo,
# crée les raccourcis « MailCraft VIP » et « Mettre à jour MailCraft » (Dock, Bureau, Applications),
# puis fabrique et installe l'appli.
#   vip-mac-setup.sh [JETON_FORGEJO]   jeton en lecture seule (Forgejo › Paramètres › Applications,
#                                      portée read:repository) ; demandé dans une fenêtre s'il manque.
set -euo pipefail
BASE="$HOME/Applications/MailCraft-VIP"; SRC="$BASE/src"
REPO="https://git.artisan-digital.eu/arnaud/mailcraft-vip.git"
mkdir -p "$BASE" "$HOME/Applications"

echo "▶ Outils Apple (Xcode Command Line Tools)…"
if ! xcode-select -p >/dev/null 2>&1; then
  xcode-select --install || true
  osascript -e 'display dialog "Une fenêtre Apple va installer les outils de développement. Clique sur « Installer », attends la fin, puis relance cette installation." with title "MailCraft VIP" buttons {"OK"}' >/dev/null
  exit 1
fi

echo "▶ Rust…"
if ! command -v cargo >/dev/null 2>&1 && [ ! -x "$HOME/.cargo/bin/cargo" ]; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
fi
. "$HOME/.cargo/env"

echo "▶ Code de MailCraft VIP (notre Forgejo)…"
if [ ! -d "$SRC/.git" ]; then
  TOKEN="${1:-}"
  [ -n "$TOKEN" ] || TOKEN=$(osascript -e 'text returned of (display dialog "Jeton Forgejo en lecture seule (demande-le à Arnaud) :" default answer "" with hidden answer with title "MailCraft VIP")')
  git config --global credential.helper osxkeychain
  printf 'protocol=https\nhost=git.artisan-digital.eu\nusername=arnaud\npassword=%s\n\n' "$TOKEN" | git credential approve
  git clone -q --branch vip-ucs "$REPO" "$SRC"
fi

echo "▶ Raccourcis…"
UPD="$HOME/Applications/Mettre à jour MailCraft.app"
rm -rf "$UPD"
osacompile -o "$UPD" \
  -e 'tell application "Terminal"' \
  -e '  activate' \
  -e '  do script "bash \"$HOME/Applications/MailCraft-VIP/src/packaging/macos/vip/vip-mac-update.sh\"; exit"' \
  -e 'end tell'
cp "$SRC/assets/app-icon/mailcraft.icns" "$UPD/Contents/Resources/applet.icns" 2>/dev/null || true
touch "$UPD"
ln -sfn "$UPD" "$HOME/Desktop/Mettre à jour MailCraft"
ln -sfn "$HOME/Applications/MailCraft VIP.app" "$HOME/Desktop/MailCraft VIP"
for app in "$HOME/Applications/MailCraft VIP.app" "$UPD"; do
  defaults read com.apple.dock persistent-apps 2>/dev/null | grep -q "$(basename "$app" | sed 's/ /%20/g')" && continue
  defaults write com.apple.dock persistent-apps -array-add "<dict><key>tile-data</key><dict><key>file-data</key><dict><key>_CFURLString</key><string>file://${app// /%20}/</string><key>_CFURLStringType</key><integer>15</integer></dict></dict></dict>"
done

echo "▶ Première fabrication (10 à 20 minutes)…"
bash "$SRC/packaging/macos/vip/vip-mac-update.sh" --force
killall Dock 2>/dev/null || true
echo "✓ Installé : « MailCraft VIP » et « Mettre à jour MailCraft » sont dans le Dock, sur le Bureau et dans Applications."
