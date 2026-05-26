#!/usr/bin/env sh
# arx CLI installer. POSIX shell.
#
# Usage:
#   curl -sSL https://raw.githubusercontent.com/arxdevs/arx/main/install.sh | sh
#
# Installs the arx CLI binary to $BIN_DIR (default: $HOME/.local/bin).
# After install, run:
#   arx setup   # on a server box
#   arx login --server https://arx.your-domain  # on a client

set -eu

REPO="arxdevs/arx"
BIN_DIR="${ARX_BIN_DIR:-$HOME/.local/bin}"
BIN_NAME="arx"

fail() { printf 'error: %s\n' "$1" >&2; exit 1; }

# OS detection.
uname_s=$(uname -s)
case "$uname_s" in
    Linux)  os="linux"  ;;
    Darwin) os="darwin" ;;
    *) fail "unsupported OS: $uname_s" ;;
esac

# Arch detection.
uname_m=$(uname -m)
case "$uname_m" in
    x86_64|amd64) arch="amd64"  ;;
    arm64|aarch64) arch="arm64" ;;
    *) fail "unsupported arch: $uname_m" ;;
esac

# Find latest release version via the GitHub redirect (no jq required).
latest_url="https://github.com/${REPO}/releases/latest"
resolved=$(curl -sSL -o /dev/null -w '%{url_effective}' "$latest_url") \
    || fail "could not reach github.com/${REPO}/releases/latest"
# resolved ends with .../releases/tag/<version>
version="${resolved##*/tag/}"
if [ "$version" = "$resolved" ] || [ -z "$version" ]; then
    fail "could not determine latest version (try setting ARX_VERSION=v0.x.y)"
fi
version="${ARX_VERSION:-$version}"

asset="arx-${os}-${arch}.tar.gz"
asset_url="https://github.com/${REPO}/releases/download/${version}/${asset}"

printf 'arx installer\n'
printf '  version: %s\n' "$version"
printf '  target:  %s-%s\n' "$os" "$arch"
printf '  url:     %s\n' "$asset_url"

# Download to temp dir.
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

if ! curl -sSL --fail -o "$tmp/$asset" "$asset_url"; then
    fail "download failed (asset may not exist for this OS/arch yet)"
fi

tar -xzf "$tmp/$asset" -C "$tmp"

mkdir -p "$BIN_DIR"
install -m 0755 "$tmp/$BIN_NAME" "$BIN_DIR/$BIN_NAME"

printf '\n✓ installed: %s\n' "$BIN_DIR/$BIN_NAME"

case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *) printf '\n! add %s to your PATH (e.g. in ~/.bashrc / ~/.zshrc):\n    export PATH="%s:$PATH"\n' "$BIN_DIR" "$BIN_DIR" ;;
esac

printf '\nNext:\n'
printf '  arx setup                                  # on your server box\n'
printf '  arx login --server https://arx.example.com # on a client\n'
