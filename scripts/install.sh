#!/bin/bash
set -e

VERSION="${AYG_VERSION:-latest}"
REPO="hemeda3/aygrep"

OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)

case "$OS" in
    linux)  OS="linux" ;;
    darwin) OS="macos" ;;
    *)      echo "Unsupported OS: $OS"; exit 1 ;;
esac

case "$ARCH" in
    x86_64|amd64) ARCH="amd64" ;;
    aarch64|arm64) ARCH="arm64" ;;
    *)             echo "Unsupported arch: $ARCH"; exit 1 ;;
esac

BINARY="ayg-${OS}-${ARCH}"

if [ "$VERSION" = "latest" ]; then
    VERSION=$(curl -sL "https://api.github.com/repos/$REPO/releases/latest" | grep tag_name | cut -d'"' -f4)
fi

echo "Installing ayg $VERSION ($OS/$ARCH)..."

URL="https://github.com/$REPO/releases/download/$VERSION/$BINARY"
curl -fsSL "$URL" -o /tmp/ayg
chmod +x /tmp/ayg

# Verify it runs
/tmp/ayg --version

# Install
if [ -w /usr/local/bin ]; then
    mv /tmp/ayg /usr/local/bin/ayg
    echo "Installed to /usr/local/bin/ayg"
else
    mkdir -p "$HOME/.local/bin"
    mv /tmp/ayg "$HOME/.local/bin/ayg"
    echo "Installed to ~/.local/bin/ayg"
    echo "Add to PATH: export PATH=\"\$HOME/.local/bin:\$PATH\""
fi
