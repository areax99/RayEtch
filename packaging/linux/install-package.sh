#!/bin/sh
set -eu

if [ "$(uname -s)" != Linux ]; then
  echo "Errore: il pacchetto può essere installato solo su Linux." >&2
  exit 1
fi

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
for FILE in rayetch icon.png document.png rayetch.desktop rayetch-mime.xml rust-target; do
  if [ ! -f "$ROOT/$FILE" ]; then
    echo "Errore: file del pacchetto mancante: $ROOT/$FILE" >&2
    exit 1
  fi
done

TARGET=$(cat "$ROOT/rust-target")
case "$TARGET" in
  x86_64-*-linux-*) ARCH=x86_64 ;;
  aarch64-*-linux-*) ARCH=aarch64 ;;
  i686-*-linux-*) ARCH=i686 ;;
  armv7-*-linux-*) ARCH=armv7 ;;
  *)
    echo "Errore: architettura del pacchetto non supportata: $TARGET" >&2
    exit 1
    ;;
esac

MACHINE=$(uname -m)
case "$MACHINE:$ARCH" in
  x86_64:x86_64|aarch64:aarch64|arm64:aarch64|i686:i686|i386:i686|armv7l:armv7)
    ;;
  *)
    echo "Errore: questo pacchetto ($ARCH) non corrisponde all'architettura ($MACHINE)." >&2
    exit 1
    ;;
esac

for tool in install sed; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Errore: $tool non trovato." >&2
    exit 1
  fi
done

BIN="$HOME/.local/bin"
ICONS="$HOME/.local/share/icons/hicolor"
APPS="$HOME/.local/share/applications"
PIXMAPS="$HOME/.local/share/pixmaps"
MIME="$HOME/.local/share/mime"
DOCS="$HOME/.local/share/doc/rayetch"
mkdir -p "$BIN" "$APPS" "$PIXMAPS" "$ICONS/1024x1024/apps" \
  "$ICONS/1024x1024/mimetypes" "$MIME/packages" "$DOCS"

install -m 755 "$ROOT/rayetch" "$BIN/rayetch"
install -m 644 "$ROOT/icon.png" "$ICONS/1024x1024/apps/rayetch.png"
install -m 644 "$ROOT/icon.png" "$PIXMAPS/rayetch.png"
install -m 644 "$ROOT/document.png" \
  "$ICONS/1024x1024/mimetypes/application-x-rayetch.png"

for SIZE in 48 64 128 256 512; do
  if command -v convert >/dev/null 2>&1; then
    mkdir -p "$ICONS/${SIZE}x${SIZE}/apps"
    convert "$ROOT/icon.png" -resize "${SIZE}x${SIZE}" \
      "$ICONS/${SIZE}x${SIZE}/apps/rayetch.png"
  fi
done
for SIZE in 48 64 128 256; do
  if command -v convert >/dev/null 2>&1; then
    mkdir -p "$ICONS/${SIZE}x${SIZE}/mimetypes"
    convert "$ROOT/document.png" -resize "${SIZE}x${SIZE}" \
      "$ICONS/${SIZE}x${SIZE}/mimetypes/application-x-rayetch.png"
  fi
done

install -m 644 "$ROOT/rayetch-mime.xml" "$MIME/packages/rayetch.xml"
install -m 644 "$ROOT/README.md" "$ROOT/LICENSE" "$ROOT/NOTICE" "$DOCS/"

EXEC_PATH=$(printf '%s\n' "$BIN/rayetch" | sed 's/\\/\\\\\\\\/g; s/["`$]/\\\\&/g; s/%/%%/g')
ICON_PATH=$(printf '%s\n' "$PIXMAPS/rayetch.png" | sed 's/\\/\\\\/g')
DESKTOP_TMP="$APPS/.rayetch.desktop.$$"
trap 'rm -f -- "$DESKTOP_TMP"' EXIT
while IFS= read -r LINE; do
  case "$LINE" in
    Exec=*) printf 'Exec="%s" %%f\n' "$EXEC_PATH" ;;
    Icon=*) printf 'Icon=%s\n' "$ICON_PATH" ;;
    *) printf '%s\n' "$LINE" ;;
  esac
done < "$ROOT/rayetch.desktop" > "$DESKTOP_TMP"
chmod 644 "$DESKTOP_TMP"
mv -f "$DESKTOP_TMP" "$APPS/rayetch.desktop"

if command -v update-mime-database >/dev/null 2>&1; then
  update-mime-database "$MIME"
else
  echo "Avviso: update-mime-database non trovato; l'associazione .rayetch potrebbe non essere aggiornata." >&2
fi
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$APPS"
else
  echo "Avviso: update-desktop-database non trovato; il menu potrebbe non aggiornarsi subito." >&2
fi

echo "RayEtch installato per l'utente corrente. Avvialo dal menu applicazioni."
