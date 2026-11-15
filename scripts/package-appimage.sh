#!/bin/sh
set -eu

if [ "$(uname -s)" != Linux ]; then
  echo "Errore: esegui questo script su Linux, non in cross-compilazione." >&2
  exit 1
fi

if [ "$#" -ne 0 ]; then
  echo "Uso: sh scripts/package-appimage.sh" >&2
  exit 1
fi

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"

for tool in cargo rustc; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Errore: $tool non trovato; installa Rust e aggiungilo al PATH." >&2
    exit 1
  fi
done

LINUXDEPLOY=${LINUXDEPLOY:-linuxdeploy}
if ! command -v "$LINUXDEPLOY" >/dev/null 2>&1; then
  echo "Errore: linuxdeploy non trovato. Imposta LINUXDEPLOY al percorso dell'eseguibile." >&2
  exit 1
fi
LINUXDEPLOY=$(command -v "$LINUXDEPLOY")
LINUXDEPLOY=$(CDPATH= cd -- "$(dirname -- "$LINUXDEPLOY")" && pwd)/$(basename -- "$LINUXDEPLOY")

HOST=$(rustc -vV | sed -n 's/^host: //p')
case "$HOST" in
  x86_64-unknown-linux-gnu) ARCH=x86_64 ;;
  aarch64-unknown-linux-gnu) ARCH=aarch64 ;;
  *)
    echo "Errore: toolchain Linux GNU x86_64 o aarch64 richiesta (host: $HOST)." >&2
    exit 1
    ;;
esac
case "$(uname -m):$ARCH" in
  x86_64:x86_64|aarch64:aarch64|arm64:aarch64) ;;
  *)
    echo "Errore: la toolchain Rust deve corrispondere all'architettura Linux corrente." >&2
    exit 1
    ;;
esac

VERSION=$(awk '
  /^\[package\]$/ { in_package = 1; next }
  /^\[/ { in_package = 0 }
  in_package && /^version[[:space:]]*=/ {
    gsub(/^[^"]*"/, "")
    sub(/".*$/, "")
    print
    exit
  }
' Cargo.toml)
case "$VERSION" in
  ""|*[!A-Za-z0-9.+-]*)
    echo "Errore: versione non valida in Cargo.toml: $VERSION" >&2
    exit 1
    ;;
esac

OUTPUT_DIR="$ROOT/release-archives"
OUTPUT="$OUTPUT_DIR/RayEtch-v$VERSION-AppImage-$ARCH.AppImage"
if [ -e "$OUTPUT" ]; then
  echo "Errore: il file esiste già: $OUTPUT" >&2
  exit 1
fi

mkdir -p "$OUTPUT_DIR"
STAGING=$(mktemp -d "$OUTPUT_DIR/.appimage-XXXXXXXX")
trap 'rm -rf -- "$STAGING"' EXIT
trap 'exit 1' HUP INT TERM

export CARGO_TARGET_DIR="$ROOT/target"
cargo build --release --locked --target "$HOST"
BIN="$CARGO_TARGET_DIR/$HOST/release/rayetch"
if [ ! -x "$BIN" ]; then
  echo "Errore: eseguibile compilato non trovato: $BIN" >&2
  exit 1
fi

cp assets/icon.png "$STAGING/rayetch.png"
mkdir -p "$STAGING/AppDir/usr/share/doc/rayetch"
cp README.md LICENSE NOTICE "$STAGING/AppDir/usr/share/doc/rayetch/"
LDAI_OUTPUT="$STAGING/RayEtch.AppImage" "$LINUXDEPLOY" \
  --appdir "$STAGING/AppDir" \
  --executable "$BIN" \
  --desktop-file "$ROOT/packaging/linux/rayetch.desktop" \
  --icon-file "$STAGING/rayetch.png" \
  --output appimage

if [ ! -s "$STAGING/RayEtch.AppImage" ]; then
  echo "Errore: linuxdeploy non ha creato l'AppImage." >&2
  exit 1
fi
chmod +x "$STAGING/RayEtch.AppImage"
if ! ln "$STAGING/RayEtch.AppImage" "$OUTPUT"; then
  echo "Errore: impossibile creare $OUTPUT; verifica che non esista già." >&2
  exit 1
fi
echo "AppImage creata: $OUTPUT"
