#!/bin/bash
# Release workflow for ayg
#
# Usage:
#   ./scripts/release.sh patch          # 0.1.0 → 0.1.1 (auto, publishes to crates.io)
#   ./scripts/release.sh minor          # 0.1.1 → 0.2.0 (auto, publishes to crates.io)
#   ./scripts/release.sh major          # 0.2.0 → 1.0.0 (auto, publishes to crates.io)
#   ./scripts/release.sh beta           # 0.1.0 → 0.1.1-beta.1 (prerelease, no crates.io)
#   ./scripts/release.sh beta           # 0.1.1-beta.1 → 0.1.1-beta.2 (bump beta)
#   ./scripts/release.sh stable         # 0.1.1-beta.2 → 0.1.1 (promote to stable)
#
# Flow:
#   beta → test internally → beta again if needed → stable when ready
#   patch/minor/major → straight to stable (for small fixes)
#
# What happens:
#   1. Bumps version in Cargo.toml + src/main.rs
#   2. Commits, tags, pushes
#   3. CI: tests on ubuntu + macos
#   4. CI: builds 4 binaries (linux/macos × amd64/arm64)
#   5. CI: uploads to GitHub Release
#   6. CI: publishes to crates.io (stable only, not beta/rc)
#
# Users on `brew install` or `cargo install` only get stable releases.
# Beta releases are GitHub-only prereleases for testing.

set -e

BUMP="${1:-patch}"

# Get current version
CURRENT=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')

# Parse version
BASE=$(echo "$CURRENT" | sed 's/-.*//')  # strip -beta.N
IFS='.' read -r MAJOR MINOR PATCH <<< "$BASE"
PRERELEASE=$(echo "$CURRENT" | grep -oP '\-.*' || echo "")

case "$BUMP" in
  patch)
    PATCH=$((PATCH + 1))
    NEW="$MAJOR.$MINOR.$PATCH"
    ;;
  minor)
    MINOR=$((MINOR + 1)); PATCH=0
    NEW="$MAJOR.$MINOR.$PATCH"
    ;;
  major)
    MAJOR=$((MAJOR + 1)); MINOR=0; PATCH=0
    NEW="$MAJOR.$MINOR.$PATCH"
    ;;
  beta)
    if echo "$CURRENT" | grep -q "beta"; then
      # Already beta — bump beta number
      BETA_NUM=$(echo "$CURRENT" | grep -oP 'beta\.\K\d+')
      BETA_NUM=$((BETA_NUM + 1))
      NEW="$BASE-beta.$BETA_NUM"
    else
      # New beta for next patch
      PATCH=$((PATCH + 1))
      NEW="$MAJOR.$MINOR.$PATCH-beta.1"
    fi
    ;;
  stable)
    if echo "$CURRENT" | grep -q "beta\|rc\|alpha"; then
      NEW="$BASE"  # strip prerelease suffix
    else
      echo "Already stable ($CURRENT). Use patch/minor/major."
      exit 1
    fi
    ;;
  *)
    echo "Usage: $0 patch|minor|major|beta|stable"
    exit 1
    ;;
esac

echo ""
echo "  $CURRENT → $NEW"
echo ""
read -p "Release v$NEW? [y/N] " -n 1 -r
echo
if [[ ! $REPLY =~ ^[Yy]$ ]]; then
    echo "Cancelled."
    exit 0
fi

# Update version in Cargo.toml
if [[ "$OSTYPE" == "darwin"* ]]; then
    sed -i '' "s/^version = \"$CURRENT\"/version = \"$NEW\"/" Cargo.toml
    sed -i '' "s/ayg $CURRENT/ayg $NEW/" src/main.rs
else
    sed -i "s/^version = \"$CURRENT\"/version = \"$NEW\"/" Cargo.toml
    sed -i "s/ayg $CURRENT/ayg $NEW/" src/main.rs
fi

# Build to verify
cargo build --release

echo ""
echo "Build OK. Pushing..."

git add -A
git commit -m "v$NEW"
git tag "v$NEW"
git push origin main "v$NEW"

echo ""
echo "v$NEW released."
if echo "$NEW" | grep -q "beta\|rc\|alpha"; then
    echo "  Prerelease — binaries on GitHub only, not on crates.io."
else
    echo "  Stable — binaries + crates.io publish."
fi
echo "  https://github.com/hemeda3/aygrep/actions"
