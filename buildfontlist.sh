#!/usr/bin/env bash
# buildfontlist.sh — Generate fontlist.json from the Google Fonts metadata.
#
# Fetches the full catalog (~2MB JSON), sorts by popularity score, and
# outputs a JSON array of family name strings.
#
# Requires: curl, jq
#
# Usage: ./buildfontlist.sh

set -euo pipefail

METADATA_URL="https://fonts.google.com/metadata/fonts"
OUTPUT="fontlist.json"

echo "Fetching Google Fonts metadata..."
RAW=$(curl -sS "$METADATA_URL")

echo "Sorting by popularity and extracting family names..."
echo "$RAW" \
    | jq '[.familyMetadataList | sort_by(.popularity) | .[].family]' \
    > "$OUTPUT"

COUNT=$(jq 'length' "$OUTPUT")
echo "Done — wrote $COUNT fonts to $OUTPUT"
