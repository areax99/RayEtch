#!/bin/sh
set -eu

if [ "$(uname -s)" != Linux ]; then
  echo "Errore: esegui questo script su Linux, non in cross-compilazione." >&2
  exit 1
fi

FORCE=0
case "$#" in
  0) ;;
  1)
    if [ "$1" = "--force" ]; then
      FORCE=1
    else
      echo "Uso: sh scripts/package-linux-installer.sh [--force]" >&2
      exit 1
    fi
    ;;
  *)
    echo "Uso: sh scripts/package-linux-installer.sh [--force]" >&2
    exit 1
    ;;
esac

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"

for tool in cargo rustc tar install mktemp; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Errore: $tool non trovato; installa gli strumenti richiesti." >&2
    exit 1
  fi
done

HOST=$(rustc -vV | sed -n 's/^host: //p')
case "$HOST" in
  x86_64-*-linux-*|aarch64-*-linux-*|i686-*-linux-*|armv7-*-linux-*)
    ARCH=${HOST%%-*}
    ABI=${HOST#*-linux-}
    ;;
  *)
    echo "Errore: è richiesta una toolchain Rust Linux supportata (host: $HOST)." >&2
    exit 1
    ;;
esac

MACHINE=$(uname -m)
case "$MACHINE:$ARCH" in
  x86_64:x86_64|aarch64:aarch64|arm64:aarch64|i686:i686|i386:i686|armv7l:armv7)
    ;;
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

CHANNEL=""
case "$VERSION" in
  *-*) ;;
  *) CHANNEL="-beta" ;;
esac

LABEL="RayEtch-v${VERSION}${CHANNEL}-linux-${ARCH}-${ABI}-installer"
OUTPUT_DIR="$ROOT/release-archives"
OUTPUT="$OUTPUT_DIR/$LABEL.tar.gz"
if [ -d "$OUTPUT" ]; then
  echo "Errore: il percorso di output è una cartella: $OUTPUT" >&2
  exit 1
fi
if { [ -e "$OUTPUT" ] || [ -L "$OUTPUT" ]; } && [ "$FORCE" -ne 1 ]; then
  echo "Errore: il file esiste già: $OUTPUT (usa --force per sostituirlo)." >&2
  exit 1
fi

export CARGO_TARGET_DIR="$ROOT/target"
cargo build --release --locked --target "$HOST"

BIN="$CARGO_TARGET_DIR/$HOST/release/rayetch"
if [ ! -x "$BIN" ]; then
  echo "Errore: eseguibile compilato non trovato: $BIN" >&2
  exit 1
fi

mkdir -p "$OUTPUT_DIR"
STAGING=$(mktemp -d "$OUTPUT_DIR/.package-linux-installer-XXXXXXXX")
trap 'rm -rf "$STAGING"' EXIT
trap 'exit 1' HUP INT TERM

PACKAGE_DIR="$STAGING/$LABEL"
mkdir -p "$PACKAGE_DIR"
install -m 755 "$BIN" "$PACKAGE_DIR/rayetch"
install -m 644 assets/icon.png "$PACKAGE_DIR/icon.png"
install -m 644 assets/document.png "$PACKAGE_DIR/document.png"
mkdir -p "$PACKAGE_DIR/document-icons"
for SIZE in 16 32 48 64 128 256; do
  install -m 644 "packaging/linux/document-icons/$SIZE.png" "$PACKAGE_DIR/document-icons/$SIZE.png"
done
install -m 644 packaging/linux/rayetch.desktop "$PACKAGE_DIR/rayetch.desktop"
install -m 644 packaging/linux/rayetch-mime.xml "$PACKAGE_DIR/rayetch-mime.xml"
install -m 755 packaging/linux/install-package.sh "$PACKAGE_DIR/install.sh"
install -m 755 packaging/linux/uninstall-package.sh "$PACKAGE_DIR/uninstall.sh"
install -m 644 README.md LICENSE NOTICE "$PACKAGE_DIR/"
printf '%s\n' "$HOST" > "$PACKAGE_DIR/rust-target"

ARCHIVE="$STAGING/$LABEL.tar.gz"
tar -czf "$ARCHIVE" -C "$STAGING" "$LABEL"
if [ ! -s "$ARCHIVE" ] || ! tar -tzf "$ARCHIVE" >/dev/null; then
  echo "Errore: impossibile creare o verificare l'archivio." >&2
  exit 1
fi

if [ "$FORCE" -eq 1 ]; then
  mv -f "$ARCHIVE" "$OUTPUT"
elif ! ln "$ARCHIVE" "$OUTPUT"; then
  echo "Errore: impossibile creare $OUTPUT; verifica che non esista già." >&2
  exit 1
fi

echo "Distribuzione creata: $OUTPUT"
echo "Estrai l'archivio ed esegui sh install.sh per installare."
