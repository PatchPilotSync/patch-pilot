#!/usr/bin/env bash
# Prints the release notes for one app version: its section of app/CHANGELOG.md,
# without the heading. Fails if the section is missing or empty, so a release
# never ships without saying what changed. Usage: release-notes.sh 0.2.0
set -euo pipefail
version="${1:?usage: release-notes.sh <version>}"
changelog="$(dirname "$0")/../../app/CHANGELOG.md"
notes="$(awk -v v="$version" '
  /^## / { if (found) exit; found = ($2 == v || $2 == "[" v "]"); next }
  found { print }
' "$changelog" | sed -e '/./,$!d' | sed -e ':a' -e '/^\n*$/{$d;N;ba' -e '}')"
if [ -z "$notes" ]; then
  echo "app/CHANGELOG.md has no notes under '## $version'" >&2
  exit 1
fi
printf '%s\n' "$notes"
