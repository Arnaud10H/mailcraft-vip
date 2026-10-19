#!/usr/bin/env bash
# Build MailCraft VIP and install it for the current user on Linux: the binaries in ~/.local/bin,
# the PNG icons in the hicolor theme (named mailcraft-vip) and a launcher whose window class
# matches the app (io.github.billsecond.mailcraft), so docks pin and group it.
#
#   packaging/vip-install.sh
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
cargo build --release -p mailcraft -p mailcraft-cli
install -Dm755 target/release/mailcraft "$HOME/.local/bin/mailcraft"
install -Dm755 target/release/mailcraft-cli "$HOME/.local/bin/mailcraft-cli"
for n in 32 48 64 128 256; do
  install -Dm644 "assets/app-icon/hicolor/${n}x${n}/apps/io.github.billsecond.mailcraft.png" \
    "$HOME/.local/share/icons/hicolor/${n}x${n}/apps/mailcraft-vip.png"
done
gtk-update-icon-cache -f -t "$HOME/.local/share/icons/hicolor" >/dev/null 2>&1 || true
apps="$HOME/.local/share/applications"
mkdir -p "$apps"
cat >"$apps/io.github.billsecond.mailcraft.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=MailCraft VIP
GenericName=Mail Client
Comment=MailCraft with the VIP Mail features
Exec=$HOME/.local/bin/mailcraft --control 7991 %U
TryExec=$HOME/.local/bin/mailcraft
Icon=mailcraft-vip
Terminal=false
StartupNotify=true
StartupWMClass=io.github.billsecond.mailcraft
Categories=Office;Email;Calendar;ContactManagement;
Keywords=mail;email;vip;inbox;calendar;
MimeType=x-scheme-handler/mailto;message/rfc822;
EOF
update-desktop-database "$apps" >/dev/null 2>&1 || true
echo "Installed: $HOME/.local/bin/mailcraft and the MailCraft VIP launcher"
