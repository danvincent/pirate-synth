#!/usr/bin/env bash
set -euo pipefail

OUT_DIR="${1:-wavetables}"
COUNT_RANDOM="${COUNT_RANDOM:-64}"
MIN_SIZE="${MIN_SIZE:-256}"
MAX_SIZE="${MAX_SIZE:-8192}"

mkdir -p "$OUT_DIR"

write_soft_random() {
  local name="$1"
  local size="$2"
  local seed="$3"

  awk -v n="$size" -v seed="$seed" '
    function clamp(x) { return (x > 1 ? 1 : (x < -1 ? -1 : x)) }
    function abs(x)   { return x < 0 ? -x : x }

    BEGIN {
      pi = atan2(0, -1)
      srand(seed)

      partials = 4 + int(rand() * 8)
      base_phase = rand() * 2 * pi

      for (p = 1; p <= partials; p++) {
        freq[p] = p + ((rand() - 0.5) * 0.08)
        amp[p] = (1.0 / (p * p)) * (0.65 + 0.35 * rand())
        phase[p] = base_phase + ((rand() - 0.5) * 0.6)
        wobble_amt[p]  = rand() * 0.08
        wobble_freq[p] = 0.2 + rand() * 1.2
        wobble_ph[p]   = rand() * 2 * pi
      }

      sub_amp   = 0.05 + rand() * 0.08
      sub_phase = rand() * 2 * pi

      drift1 = rand() * 2 * pi
      drift2 = rand() * 2 * pi

      maxabs = 0

      for (i = 0; i < n; i++) {
        t = i / n
        v = 0

        for (p = 1; p <= partials; p++) {
          wob = 1.0 + wobble_amt[p] * sin(2 * pi * wobble_freq[p] * t + wobble_ph[p])
          v += (amp[p] * wob) * sin(2 * pi * freq[p] * t + phase[p])
        }

        v += sub_amp * sin(2 * pi * 0.5 * t + sub_phase)

        jitter = 0.035 * sin(2 * pi * 3.1 * t + drift1) + 0.020 * sin(2 * pi * 6.7 * t + drift2) + 0.010 * sin(2 * pi * 11.3 * t + 0.7 * drift1)

        v += jitter
        v = v / (1 + 0.6 * abs(v))

        tmp[i] = v
        if (abs(v) > maxabs) maxabs = abs(v)
      }

      if (maxabs < 1e-9) maxabs = 1

      for (i = 0; i < n; i++) {
        v = 0.92 * (tmp[i] / maxabs)
        print clamp(v)
      }
    }
  ' > "$OUT_DIR/$name.txt"

  echo "wrote $OUT_DIR/$name.txt ($size samples)"
}

for idx in $(seq 1 "$COUNT_RANDOM"); do
  size=$(( MIN_SIZE + (RANDOM * RANDOM) % (MAX_SIZE - MIN_SIZE + 1) ))
  seed=$(( 5000 + idx * 131 + size ))
  write_soft_random "soft_random_$(printf '%02d' "$idx")_${size}" "$size" "$seed"
done
