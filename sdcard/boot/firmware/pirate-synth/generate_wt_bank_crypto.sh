#!/usr/bin/env bash
# generate_wt_bank_crypto.sh — Binance public kline close-price wavetables
# Fetches hourly close prices for BTC, ETH, and SOL for the last 7 days.
# No API key required. Uses the public Binance REST v3 endpoint.
#
# Usage: ./generate_wt_bank_crypto.sh [OUTPUT_DIR]
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

fetch_klines() {
  local name="$1"
  local symbol="$2"
  local interval="${3:-1h}"
  local limit="${4:-168}"   # 168h = 7 days at 1h interval
  echo "Fetching $symbol klines ($interval × $limit)..."
  local json
  json=$(curl -sf --max-time 20 \
    "https://api.binance.com/api/v3/klines?symbol=${symbol}&interval=${interval}&limit=${limit}" \
    || echo "")

  if [[ -z "$json" ]]; then
    echo "  $name: fetch failed, skipping"
    return
  fi

  # Each kline: [open_time, open, high, low, close, volume, ...]
  # Index 4 = close price
  echo "$json" | python3 -c "
import sys, json
data = json.load(sys.stdin)
for kline in data:
    try:
        print(kline[4])
    except (IndexError, TypeError):
        pass
" | normalise_and_write "$name" \
    || echo "  $name: skipped (insufficient data)"
}

fetch_klines "crypto_btc" "BTCUSDT"
fetch_klines "crypto_eth" "ETHUSDT"
fetch_klines "crypto_sol" "SOLUSDT"

echo "Done. Wavetables written to: $OUT_DIR"
