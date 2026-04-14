#!/usr/bin/env bash
# generate_wt_bank_live.sh — fetch real-world HTTP data and convert to wavetables
# Sources:
#   - NOAA tidal gauge readings (24h water level, ~144 samples)
#   - NOAA solar flux index (observed solar cycle indices, ~300 samples)
#   - Coinbase BTC/USD historic prices (last day, variable samples)
#
# Usage:
#   ./generate_wt_bank_live.sh [OUTPUT_DIR]
#   OUTPUT_DIR defaults to ./wavetables
#
# Requirements: curl, python3 (stdlib only)
# Each successful fetch writes one .txt file. Failures are skipped with a warning.

set -euo pipefail

OUT_DIR="${1:-wavetables}"
mkdir -p "$OUT_DIR"

# ---------------------------------------------------------------------------
# Helper: normalise a newline-separated list of floats to [-1, 1] and write
# a .txt file.  Accepts values on stdin.
# ---------------------------------------------------------------------------
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

# ---------------------------------------------------------------------------
# 1. NOAA tidal gauge — 24h water level at Seattle (station 9447130)
# ---------------------------------------------------------------------------
echo "Fetching NOAA tidal data..."
tidal_json=$(curl -sf --max-time 15 \
  "https://api.tidesandcurrents.noaa.gov/api/prod/datagetter?product=water_level&station=9447130&datum=MLLW&time_zone=gmt&units=metric&format=json&date=recent" \
  || echo "")

if [[ -n "$tidal_json" ]]; then
  echo "$tidal_json" | python3 -c "
import sys, json
data = json.load(sys.stdin)
readings = data.get('data', [])
for r in readings:
    v = r.get('v', '').strip()
    if v:
        print(v)
" | normalise_and_write "tidal_seattle" \
    && echo "  tidal: OK" \
    || echo "  tidal: skipped (insufficient data)"
else
  echo "  tidal: fetch failed, skipping"
fi

# ---------------------------------------------------------------------------
# 2. NOAA solar flux — observed solar cycle smoothed monthly flux (F10.7)
# ---------------------------------------------------------------------------
echo "Fetching NOAA solar flux..."
solar_json=$(curl -sf --max-time 15 \
  "https://services.swpc.noaa.gov/json/solar-cycle/observed-solar-cycle-indices.json" \
  || echo "")

if [[ -n "$solar_json" ]]; then
  echo "$solar_json" | python3 -c "
import sys, json
data = json.load(sys.stdin)
for entry in data:
    v = entry.get('f10.7', None)
    if v is not None and str(v).strip() not in ('', '-1'):
        print(v)
" | normalise_and_write "solar_flux" \
    && echo "  solar: OK" \
    || echo "  solar: skipped (insufficient data)"
else
  echo "  solar: fetch failed, skipping"
fi

# ---------------------------------------------------------------------------
# 3. Coinbase BTC/USD historic prices — hourly for last 24h
# ---------------------------------------------------------------------------
echo "Fetching Coinbase BTC/USD prices..."
btc_json=$(curl -sf --max-time 15 \
  "https://api.coinbase.com/v2/prices/BTC-USD/historic?period=day" \
  || echo "")

if [[ -n "$btc_json" ]]; then
  echo "$btc_json" | python3 -c "
import sys, json
data = json.load(sys.stdin)
prices = data.get('data', {}).get('prices', [])
for p in prices:
    v = p.get('price', '').strip()
    if v:
        print(v)
" | normalise_and_write "btc_price" \
    && echo "  btc: OK" \
    || echo "  btc: skipped (insufficient data)"
else
  echo "  btc: fetch failed, skipping"
fi

echo "Done. Wavetables written to: $OUT_DIR"
