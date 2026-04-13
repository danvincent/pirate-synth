#!/usr/bin/env bash
# generate_wt_bank_tidal.sh — NOAA tidal gauge water level wavetables
# Fetches 24h of water level readings from three US coastal stations.
# Outputs one .wt file per station.
#
# Usage: ./generate_wt_bank_tidal.sh [OUTPUT_DIR]
# Requires: curl, python3

set -euo pipefail

OUT_DIR="${1:-wavetables}"
mkdir -p "$OUT_DIR"

normalise_and_write() {
  local name="$1"
  OUT_PATH="$OUT_DIR/$name.wt" python3 -c "
import sys, os
out_path = os.environ['OUT_PATH']
vals = []
for line in sys.stdin:
    line = line.strip()
    if line:
        try:
            vals.append(float(line))
        except ValueError:
            pass
if len(vals) < 2:
    print('  skip ' + out_path + ': fewer than 2 usable samples', file=sys.stderr)
    sys.exit(1)
lo, hi = min(vals), max(vals)
span = hi - lo
if span < 1e-9:
    span = 1.0
normalised = [(v - lo) / span * 2.0 - 1.0 for v in vals]
with open(out_path, 'w') as f:
    for x in normalised:
        x = max(-1.0, min(1.0, x))
        f.write(f'{x:.6f}\n')
print(f'  wrote {out_path} ({len(normalised)} samples)')
"
}

fetch_station() {
  local name="$1"
  local station="$2"
  echo "Fetching tidal station $station ($name)..."
  local json
  json=$(curl -sf --max-time 20 \
    "https://api.tidesandcurrents.noaa.gov/api/prod/datagetter?product=water_level&station=${station}&datum=MLLW&time_zone=gmt&units=metric&format=json&date=recent" \
    || echo "")

  if [[ -z "$json" ]]; then
    echo "  $name: fetch failed, skipping"
    return
  fi

  echo "$json" | python3 -c "
import sys, json
data = json.load(sys.stdin)
for r in data.get('data', []):
    v = r.get('v', '').strip()
    if v:
        print(v)
" | normalise_and_write "$name" \
    || echo "  $name: skipped (insufficient data)"
}

fetch_station "tidal_seattle"   "9447130"
fetch_station "tidal_newyork"   "8518750"
fetch_station "tidal_keywest"   "8724580"

echo "Done. Wavetables written to: $OUT_DIR"
