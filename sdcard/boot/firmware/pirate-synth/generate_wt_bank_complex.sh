#!/usr/bin/env bash
set -euo pipefail

OUT_DIR="${1:-wavetables}"
COUNT_RANDOM="${COUNT_RANDOM:-64}"
MIN_SIZE="${MIN_SIZE:-256}"
MAX_SIZE="${MAX_SIZE:-8192}"

mkdir -p "$OUT_DIR"

rand_size() {
  awk -v min="$MIN_SIZE" -v max="$MAX_SIZE" 'BEGIN {
    srand()
    print int(min + rand() * (max - min + 1))
  }'
}

write_table() {
  local name="$1"
  local size="$2"
  local expr="$3"

  awk -v n="$size" '
    function clamp(x) {
      return (x > 1 ? 1 : (x < -1 ? -1 : x))
    }
    function abs(x) {
      return x < 0 ? -x : x
    }
    BEGIN {
      pi = atan2(0, -1)
      for (i = 0; i < n; i++) {
        t = i / n
        '"$expr"'
        print clamp(v)
      }
    }
  ' > "$OUT_DIR/$name.txt"

  echo "wrote $OUT_DIR/$name.txt ($size samples)"
}

# A few deterministic shapes with random sizes
size="$(rand_size)"
write_table "sine_${size}" "$size" '
  v = sin(2 * pi * t)
'

size="$(rand_size)"
write_table "triangle_${size}" "$size" '
  v = 2 * abs(2 * t - 1) - 1
'

size="$(rand_size)"
write_table "folded_${size}" "$size" '
  x = 1.2 * sin(2*pi*1*t) + 0.7 * sin(2*pi*2*t) + 0.3 * sin(2*pi*5*t)
  if (x > 1) x = 2 - x
  if (x < -1) x = -2 - x
  v = x
'

size="$(rand_size)"
write_table "metallic_${size}" "$size" '
  v = 0.90 * sin(2*pi*1.0*t) \
    + 0.60 * sin(2*pi*2.71*t) \
    + 0.45 * sin(2*pi*4.13*t) \
    + 0.30 * sin(2*pi*7.37*t)
  v /= 2.25
'

# Random complex tables with random sizes
for idx in $(seq 1 "$COUNT_RANDOM"); do
  size="$(rand_size)"
  seed=$((2000 + idx * 97 + size))

  awk -v n="$size" -v seed="$seed" '
    function clamp(x) {
      return (x > 1 ? 1 : (x < -1 ? -1 : x))
    }
    function abs(x) {
      return x < 0 ? -x : x
    }
    BEGIN {
      pi = atan2(0, -1)
      srand(seed)

      partials = 6 + int(rand() * 24)

      for (p = 1; p <= partials; p++) {
        freq[p]  = 0.25 + rand() * 40.0
        amp[p]   = 0.03 + rand() * 1.2
        phase[p] = rand() * 2 * pi
      }

      shape_mode = int(rand() * 6)

      for (i = 0; i < n; i++) {
        t = i / n
        v = 0

        for (p = 1; p <= partials; p++) {
          v += amp[p] * sin(2 * pi * freq[p] * t + phase[p])
        }

        # broad contour across the table
        env = 0.55 + 0.45 * sin(2 * pi * t + 0.37 * seed)
        v *= env

        if (shape_mode == 0) {
          v = v / (1 + abs(v))                  # soft saturation
        } else if (shape_mode == 1) {
          if (v > 1) v = 2 - v                 # folding
          if (v < -1) v = -2 - v
        } else if (shape_mode == 2) {
          steps = 8 + int(rand() * 24)         # quantized/stepped
          v = int(((v + 1) / 2) * steps) / steps
          v = 2 * v - 1
        } else if (shape_mode == 3) {
          if (v > 0) v *= 0.65                 # asymmetry
          else v *= 1.25
        } else if (shape_mode == 4) {
          v += 0.2 * sin(2 * pi * (2 + rand() * 10) * t)
          v = v / (1 + abs(v))
        } else {
          v = sin(v * 2.5)                     # nonlinear phase-ish distortion
        }

        print clamp(v)
      }
    }
  ' > "$OUT_DIR/complex_$(printf '%02d' "$idx")_${size}.txt"

  echo "wrote $OUT_DIR/complex_$(printf '%02d' "$idx")_${size}.txt ($size samples)"
done
