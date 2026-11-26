#!/bin/sh
set -eu

if [ "$(uname -s)" != Linux ]; then
  echo "Errore: esegui questo script su Linux." >&2
  exit 1
fi

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
for FILE in install.sh rayetch rust-target icon.png document.png rayetch.desktop \
  rayetch-mime.xml README.md LICENSE NOTICE; do
  if [ ! -f "$ROOT/$FILE" ]; then
    echo "Errore: file del pacchetto mancante: $ROOT/$FILE; estrai l'intero archivio." >&2
    exit 1
  fi
done
for SIZE in 16 32 48 64 128 256; do
  if [ ! -f "$ROOT/document-icons/$SIZE.png" ]; then
    echo "Errore: icona documento mancante: $ROOT/document-icons/$SIZE.png." >&2
    exit 1
  fi
done
if [ ! -x "$ROOT/rayetch" ]; then
  echo "Errore: il file $ROOT/rayetch non è eseguibile." >&2
  exit 1
fi
if [ "$(id -u)" -eq 0 ]; then
  echo "Errore: esegui lo script come utente normale, senza sudo; sudo sarà richiesto per le dipendenze." >&2
  exit 1
fi
if [ ! -r /etc/os-release ]; then
  echo "Errore: impossibile identificare la distribuzione (/etc/os-release non disponibile)." >&2
  exit 1
fi
if ! command -v sudo >/dev/null 2>&1 || ! command -v ldd >/dev/null 2>&1; then
  echo "Errore: sono richiesti sudo e ldd." >&2
  exit 1
fi

TARGET=$(cat "$ROOT/rust-target")
case "$(uname -m):$TARGET" in
  x86_64:x86_64-*-linux-gnu|aarch64:aarch64-*-linux-gnu|arm64:aarch64-*-linux-gnu|i686:i686-*-linux-gnu|i386:i686-*-linux-gnu|armv7l:armv7-*-linux-gnueabihf)
    ;;
  *)
    echo "Errore: architettura o ABI non supportata per questo pacchetto: $(uname -m) / $TARGET." >&2
    exit 1
    ;;
esac

. /etc/os-release
DISTRO=""
for NAME in "${ID:-}" ${ID_LIKE:-}; do
  case "$NAME" in
    debian|ubuntu) DISTRO=debian; break ;;
    fedora) DISTRO=fedora; break ;;
    arch) DISTRO=arch; break ;;
    opensuse|opensuse-*|suse) DISTRO=opensuse; break ;;
  esac
done

case "$DISTRO" in
  debian)
    MANAGER=apt-get
    if ! command -v apt-cache >/dev/null 2>&1; then
      echo "Errore: apt-cache non disponibile." >&2
      exit 1
    fi
    if apt-cache show libgtk-3-0t64 >/dev/null 2>&1; then
      GTK=libgtk-3-0t64
    elif apt-cache show libgtk-3-0 >/dev/null 2>&1; then
      GTK=libgtk-3-0
    else
      echo "Errore: pacchetto GTK3 non trovato negli indici APT; aggiorna gli indici e riprova." >&2
      exit 1
    fi
    # the web tool window (Strumenti Web) uses WebKitGTK 4.1
    if apt-cache show libwebkit2gtk-4.1-0 >/dev/null 2>&1; then
      WEBKIT=libwebkit2gtk-4.1-0
    else
      echo "Errore: pacchetto libwebkit2gtk-4.1-0 non trovato negli indici APT (serve Ubuntu 22.04 / Debian 12 o più recenti)." >&2
      exit 1
    fi
    set -- "$GTK" "$WEBKIT" libudev1 libgl1 libegl1 libgl1-mesa-dri libxkbcommon0 \
      libwayland-client0 libx11-6 shared-mime-info desktop-file-utils xdg-desktop-portal
    ;;
  fedora)
    MANAGER=dnf
    set -- gtk3 webkit2gtk4.1 systemd-libs libglvnd-glx libglvnd-egl mesa-dri-drivers \
      libxkbcommon libX11 shared-mime-info desktop-file-utils xdg-desktop-portal
    ;;
  arch)
    MANAGER=pacman
    set -- gtk3 webkit2gtk-4.1 systemd-libs libglvnd mesa libxkbcommon wayland libx11 \
      shared-mime-info desktop-file-utils xdg-desktop-portal
    ;;
  opensuse)
    MANAGER=zypper
    set -- libgtk-3-0 libwebkit2gtk-4_1-0 libudev1 Mesa-libGL1 Mesa-libEGL1 Mesa-dri libxkbcommon0 \
      libwayland-client0 libX11-6 shared-mime-info desktop-file-utils xdg-desktop-portal
    ;;
  *)
    echo "Errore: distribuzione non supportata: ${ID:-sconosciuta}." >&2
    exit 1
    ;;
esac

if ! command -v "$MANAGER" >/dev/null 2>&1; then
  echo "Errore: gestore dei pacchetti $MANAGER non disponibile." >&2
  exit 1
fi

printf 'Distribuzione: %s\nPacchetti di sistema richiesti:' "$DISTRO"
printf ' %s' "$@"
printf '\nI pacchetti saranno installati con sudo e resteranno dopo la disinstallazione di RayEtch.\n'
printf 'Continuare? [s/N] '
IFS= read -r ANSWER || ANSWER=""
case "$ANSWER" in
  s|S|si|Si|SI|y|Y|yes|YES) ;;
  *) echo "Installazione annullata."; exit 1 ;;
esac

case "$DISTRO" in
  debian) sudo apt-get install "$@" ;;
  fedora) sudo dnf install "$@" ;;
  arch) sudo pacman -S --needed "$@" ;;
  opensuse) sudo zypper install "$@" ;;
esac

if ! LDD_OUTPUT=$(LC_ALL=C ldd "$ROOT/rayetch" 2>&1); then
  printf 'Errore: verifica delle librerie fallita:\n%s\n' "$LDD_OUTPUT" >&2
  exit 1
fi
case "$LDD_OUTPUT" in
  *"not found"*)
    printf 'Errore: librerie mancanti o incompatibili:\n%s\n' "$LDD_OUTPUT" >&2
    exit 1
    ;;
esac

exec sh "$ROOT/install.sh"
