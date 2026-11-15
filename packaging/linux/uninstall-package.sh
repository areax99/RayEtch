#!/bin/sh
set -eu

if [ "$(uname -s)" != Linux ]; then
  echo "Errore: il pacchetto può essere disinstallato solo su Linux." >&2
  exit 1
fi

BIN="$HOME/.local/bin"
ICONS="$HOME/.local/share/icons/hicolor"
APPS="$HOME/.local/share/applications"
PIXMAPS="$HOME/.local/share/pixmaps"
MIME="$HOME/.local/share/mime"
DOCS="$HOME/.local/share/doc/rayetch"

rm -f -- "$BIN/rayetch" "$APPS/rayetch.desktop" \
  "$PIXMAPS/rayetch.png" \
  "$ICONS/1024x1024/apps/rayetch.png" \
  "$ICONS/1024x1024/mimetypes/application-x-rayetch.png" \
  "$MIME/packages/rayetch.xml" \
  "$DOCS/README.md" "$DOCS/LICENSE" "$DOCS/NOTICE"
for SIZE in 48 64 128 256 512; do
  rm -f -- "$ICONS/${SIZE}x${SIZE}/apps/rayetch.png"
done
for SIZE in 16 32 48 64 128 256; do
  rm -f -- "$ICONS/${SIZE}x${SIZE}/mimetypes/application-x-rayetch.png"
done

if command -v update-mime-database >/dev/null 2>&1; then
  update-mime-database "$MIME"
else
  echo "Avviso: update-mime-database non trovato; la cache MIME potrebbe restare obsoleta." >&2
fi
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$APPS"
else
  echo "Avviso: update-desktop-database non trovato; la cache del menu potrebbe restare obsoleta." >&2
fi
if [ -f "$ICONS/index.theme" ] && command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -f -q "$ICONS"
fi

echo "RayEtch disinstallato per l'utente corrente."
