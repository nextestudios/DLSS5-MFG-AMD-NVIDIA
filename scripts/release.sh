#!/usr/bin/env bash
# Prepares a release: bumps the version, moves [Unreleased] in CHANGELOG.md to the new version,
# runs the tests, commits and creates the tag. Pushing is left to you (printed at the end).
# Usage: scripts/release.sh 1.1.0        (or 1.1.0-rc.1 for a pre-release)
set -euo pipefail
cd "$(dirname "$0")/.."

v="${1:-}"
[[ "$v" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$ ]] || { echo "uso: $0 X.Y.Z[-rc.N]" >&2; exit 1; }
[[ -z "$(git status --porcelain)" ]] || { echo "há mudanças não commitadas" >&2; exit 1; }
git rev-parse -q --verify "refs/tags/v$v" >/dev/null && { echo "a tag v$v já existe" >&2; exit 1; }
if [[ "$v" != *-* && "$(git branch --show-current)" != "main" ]]; then
  echo "versões finais saem da main (pré-releases podem sair de qualquer branch)" >&2; exit 1
fi

notes="$(awk '/^## \[Unreleased\]/{f=1;next} /^## \[/ || /^\[[^]]+\]: /{f=0} f' CHANGELOG.md | grep -v '^\s*$' || true)"
[[ -n "$notes" ]] || { echo "CHANGELOG.md: a seção [Unreleased] está vazia" >&2; exit 1; }

prev="$(git describe --tags --abbrev=0 --exclude '*-*' 2>/dev/null || echo "")"
today="$(date +%Y-%m-%d)"
repo="https://github.com/nextestudios/DLSS5-MFG-AMD-NVIDIA"

# Cargo.toml + Cargo.lock
perl -0pi -e "s/^version = \"[^\"]+\"/version = \"$v\"/m" Cargo.toml
cargo update -p gpu-mods-app --offline -q 2>/dev/null || cargo check -q

# CHANGELOG: new empty [Unreleased], dated section, compare links.
# Pre-releases leave it alone: their notes come from [Unreleased] until the final version.
[[ "$v" == *-* ]] || perl -0pi -e "s/^## \[Unreleased\]\n/## [Unreleased]\n\n## [$v] - $today\n/m" CHANGELOG.md
[[ "$v" == *-* ]] || perl -0pi -e "s|^\[Unreleased\]: .*\$|[Unreleased]: $repo/compare/v$v...HEAD\n[$v]: $repo/compare/${prev:-v$v}...v$v|m" CHANGELOG.md

cargo test -q
git add Cargo.toml Cargo.lock CHANGELOG.md
git commit -q -m "release: v$v"
git tag -a "v$v" -m "v$v"
echo "Pronto. Para publicar:  git push && git push origin v$v"
