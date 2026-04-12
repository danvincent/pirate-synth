#!/usr/bin/env bash
# generate_wt_bank_solar.sh — NOAA solar activity wavetables
# Sources:
#   - Observed solar cycle F10.7 flux index
#   - Observed smoothed sunspot number (SSN)
#   - Planetary Kp geomagnetic index (last 30 days)
#
# Usage: ./generate_wt_bank_solar.sh [OUTPUT_DIR]
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

# F10.7 solar flux and smoothed sunspot number from observed solar cycle indices
echo "Fetching solar cycle indices..."
cycle_json=$(curl -sf --max-time 20 \
  "https://services.swpc.noaa.gov/json/solar-cycle/observed-solar-cycle-indices.json" \
  || echo "")

if [[ -n "$cycle_json" ]]; then
  echo "$cycle_json" | python3 -c "
import sys, json
data = json.load(sys.stdin)
for e in data:
    v = e.get('f10.7')
    if v is not None and str(v).strip() not in ('', '-1'):
        print(v)
" | normalise_and_write "solar_f107" \
    || echo "  solar_f107: skipped (insufficient data)"

  echo "$cycle_json" | python3 -c "
import sys, json
data = json.load(sys.stdin)
for e in data:
    v = e.get('smoothed_ssn')
    if v is not None and str(v).strip() not in ('', '-1'):
        print(v)
" | normalise_and_write "solar_ssn" \
    || echo "  solar_ssn: skipped (insufficient data)"
else
  echo "  solar cycle: fetch failed, skipping"
fi

# Planetary Kp index — 3-hourly geomagnetic activity, last ~30 days
echo "Fetching Kp index..."
kp_json=$(curl -sf --max-time 20 \
  "https://services.swpc.noaa.gov/products/noaa-planetary-k-index.json" \
  || echo "")

if [[ -n "$kp_json" ]]; then
  echo "$kp_json" | python3 -c "
import sys, json
data = json.load(sys.stdin)
# First row is the header; skip it
for row in data[1:]:
    try:
        kp = float(row[1])
        print(kp)
    except (IndexError, TypeError, ValueError):
        pass
" | normalise_and_write "solar_kp" \
    || echo "  solar_kp: skipped (insufficient data)"
else
  echo "  solar_kp: fetch failed, skipping"
fi

echo "Done. Wavetables written to: $OUT_DIR"
