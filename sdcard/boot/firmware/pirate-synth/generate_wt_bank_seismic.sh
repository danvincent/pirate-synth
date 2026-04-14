#!/usr/bin/env bash
# generate_wt_bank_seismic.sh — USGS earthquake wavetables
# Sources:
#   - Recent M1+ earthquake magnitudes (last 7 days)
#   - Recent M1+ earthquake depths (last 7 days)
#   - Significant events of the last 30 days, by magnitude
#
# Usage: ./generate_wt_bank_seismic.sh [OUTPUT_DIR]
# Requires: curl, python3

set -euo pipefail

OUT_DIR="${1:-wavetables}"
mkdir -p "$OUT_DIR"

normalise_and_write() {
  local name="$1"
  OUT_PATH="$OUT_DIR/$name.txt" python3 -c "
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

# Recent M1+ events — last 7 days, ordered by time
echo "Fetching recent seismic events (7d M1+)..."
recent_json=$(curl -sf --max-time 30 \
  "https://earthquake.usgs.gov/fdsnws/event/1/query?format=geojson&minmagnitude=1&orderby=time&limit=512" \
  || echo "")

if [[ -n "$recent_json" ]]; then
  echo "$recent_json" | python3 -c "
import sys, json
data = json.load(sys.stdin)
for f in data.get('features', []):
    mag = f.get('properties', {}).get('mag')
    if mag is not None:
        print(mag)
" | normalise_and_write "seismic_mag" \
    || echo "  seismic_mag: skipped (insufficient data)"

  echo "$recent_json" | python3 -c "
import sys, json
data = json.load(sys.stdin)
for f in data.get('features', []):
    coords = f.get('geometry', {}).get('coordinates', [])
    if len(coords) >= 3 and coords[2] is not None:
        print(coords[2])
" | normalise_and_write "seismic_depth" \
    || echo "  seismic_depth: skipped (insufficient data)"
else
  echo "  seismic recent: fetch failed, skipping"
fi

# Significant events — last 30 days
echo "Fetching significant seismic events (30d)..."
sig_json=$(curl -sf --max-time 30 \
  "https://earthquake.usgs.gov/fdsnws/event/1/query?format=geojson&minmagnitude=4.5&orderby=time&limit=256" \
  || echo "")

if [[ -n "$sig_json" ]]; then
  echo "$sig_json" | python3 -c "
import sys, json
data = json.load(sys.stdin)
for f in data.get('features', []):
    mag = f.get('properties', {}).get('mag')
    if mag is not None:
        print(mag)
" | normalise_and_write "seismic_significant" \
    || echo "  seismic_significant: skipped (insufficient data)"
else
  echo "  seismic significant: fetch failed, skipping"
fi

echo "Done. Wavetables written to: $OUT_DIR"
