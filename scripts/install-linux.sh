#!/bin/sh
# Builds RayEtch and installs it for the current user, with its icon in the applications menu.
# Usage, from the app folder on Linux:  sh scripts/install-linux.sh
set -e
cd "$(dirname "$0")/.."

cargo build --release

BIN="$HOME/.local/bin"
ICONS="$HOME/.local/share/icons/hicolor"
APPS="$HOME/.local/share/applications"
PIXMAPS="$HOME/.local/share/pixmaps"
mkdir -p "$BIN" "$APPS" "$PIXMAPS"

# the program
install -m 755 target/release/rayetch "$BIN/rayetch"

# the icon in the sizes desktops look for (ImageMagick's "convert" if present, else 1024 px only)
for S in 48 64 128 256 512; do
  mkdir -p "$ICONS/${S}x${S}/apps"
  if command -v convert >/dev/null 2>&1; then
    convert assets/icon.png -resize ${S}x${S} "$ICONS/${S}x${S}/apps/rayetch.png"
  fi
done
mkdir -p "$ICONS/1024x1024/apps"
cp assets/icon.png "$ICONS/1024x1024/apps/rayetch.png"
install -m 644 assets/icon.png "$PIXMAPS/rayetch.png"

# the icon of saved projects (.rayetch files) and their file type
MIME="$HOME/.local/share/mime"
mkdir -p "$ICONS/1024x1024/mimetypes" "$MIME/packages"
cp assets/document.png "$ICONS/1024x1024/mimetypes/application-x-rayetch.png"
for S in 16 32 48 64 128 256; do
  mkdir -p "$ICONS/${S}x${S}/mimetypes"
  install -m 644 "packaging/linux/document-icons/$S.png" "$ICONS/${S}x${S}/mimetypes/application-x-rayetch.png"
done
install -m 644 packaging/linux/rayetch-mime.xml "$MIME/packages/rayetch.xml"
if command -v update-mime-database >/dev/null 2>&1; then
  update-mime-database "$MIME"
else
  echo "Warning: update-mime-database not found; .rayetch file associations may not be updated." >&2
fi

# the menu entry
# Escape both desktop-entry strings and Exec arguments, including literal field-code markers.
EXEC_PATH=$(printf '%s\n' "$BIN/rayetch" | sed 's/\\/\\\\\\\\/g; s/["`$]/\\\\&/g; s/%/%%/g')
ICON_PATH=$(printf '%s\n' "$PIXMAPS/rayetch.png" | sed 's/\\/\\\\/g')
while IFS= read -r LINE; do
  case "$LINE" in
    Exec=*) printf 'Exec="%s" %%f\n' "$EXEC_PATH" ;;
    Icon=*) printf 'Icon=%s\n' "$ICON_PATH" ;;
    *) printf '%s\n' "$LINE" ;;
  esac
done < packaging/linux/rayetch.desktop > "$APPS/rayetch.desktop"
chmod 644 "$APPS/rayetch.desktop"
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$APPS" || true
if [ -f "$ICONS/index.theme" ] && command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -f -q "$ICONS"
fi

# access to USB serial lasers without the dialout group or logging out (asks for the password
# once; skipped if already there). RayEtch also offers this itself the first time a port is refused.
RULE=/etc/udev/rules.d/70-rayetch-laser.rules
if [ ! -f "$RULE" ] && command -v sudo >/dev/null 2>&1; then
  echo "RayEtch needs permission to use USB lasers; your password may be asked."
  if sudo install -m 644 packaging/linux/70-rayetch-laser.rules "$RULE"; then
    sudo udevadm control --reload-rules || true
    sudo udevadm trigger --subsystem-match=tty --action=add || true
  else
    echo "Laser access not set up now; RayEtch will offer it when it finds the laser." >&2
  fi
fi

echo "Installed: $BIN/rayetch — RayEtch is in the applications menu."
echo "If 'rayetch' is not found in a terminal, add ~/.local/bin to your PATH."
