#!/bin/bash
# Usage: ./scripts/release.sh patch|minor|major
# Bumps version, commits, tags, pushes. CI builds binaries automatically.
set -e

BUMP="${1:-patch}"

# Get current version from Cargo.toml
CURRENT=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
IFS='.' read -r MAJOR MINOR PATCH <<< "$CURRENT"

case "$BUMP" in
  patch) PATCH=$((PATCH + 1)) ;;
  minor) MINOR=$((MINOR + 1)); PATCH=0 ;;
  major) MAJOR=$((MAJOR + 1)); MINOR=0; PATCH=0 ;;
  *) echo "Usage: $0 patch|minor|major"; exit 1 ;;
esac

NEW="$MAJOR.$MINOR.$PATCH"
echo "Bumping $CURRENT → $NEW"

# Update Cargo.toml
sed -i '' "s/^version = \"$CURRENT\"/version = \"$NEW\"/" Cargo.toml 2>/dev/null || \
sed -i "s/^version = \"$CURRENT\"/version = \"$NEW\"/" Cargo.toml

# Update version string in main.rs
sed -i '' "s/ayg $CURRENT/ayg $NEW/" src/main.rs 2>/dev/null || \
sed -i "s/ayg $CURRENT/ayg $NEW/" src/main.rs

# Build to verify
cargo build --release

# Commit and tag
git add -A
git commit -m "v$NEW"
git tag "v$NEW"
git push origin main "v$NEW"

echo ""
echo "Released v$NEW"
echo "CI will build binaries and create the release automatically."
echo "https://github.com/hemeda3/aygrep/releases/tag/v$NEW"
