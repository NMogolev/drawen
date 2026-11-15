# Software fill baseline

Reproducibility record for the scanline fill numbers quoted in the review and
release notes. Re-run the benchmark before claiming a regression.

```powershell
cargo run --release -p drawen-software --example fill_benchmark
```

## Machine

| Item | Value |
| --- | --- |
| CPU | AMD Ryzen 5 8400F, 6 cores / 12 threads, 4.2 GHz max |
| L3 cache | 16 MiB |
| Memory | 16 GiB (2 x 8 GiB) Kingston KF552C40-8, DDR5-4800 |
| OS | Windows, x86-64 |
| Rust | rustc 1.97.1 (8bab26f4f 2026-07-14), cargo 1.97.1 |
| `RUSTFLAGS` | unset (no `target-cpu=native`, no `-C opt-level` override) |
| Profile | workspace `release` defaults |

## Method

- One `PixelBuffer` is reused for each resolution, so the framebuffer stays
  warm and the fill scenarios run over the previous frame (steady state).
- One untimed render, then 8 warm-up renders, then 120 measured renders.
- Reported values are the median; p99 is taken from the same sorted samples.
- The text scenario is 40 lines of 16 px Ubuntu Light over opaque white.
- The clipped scenario uses a left-half `PushClip` over a full-canvas rounded
  fill, i.e. only the left half of the pixels is written.
- Hardware counters, cold-cache runs, and GPU timings are not part of this
  record.

## Cache state

The 1920x1080 RGBA buffer is 8.29 MB and fits the 16 MiB L3. The 3840x2160
buffer is 33.18 MB and does not, so the 4K numbers are DRAM-bound. That is why
ns/px roughly doubles at 4K; it is a cache cliff, not a worse code path.

## Results

Measured 2026-11-15 on the machine above. Run-to-run variance on this Windows
desktop is about 10-20%: repeated 1080p opaque runs ranged from 0.144 ms to
0.168 ms, and text from 1.26 ms to 1.66 ms. The table is one representative
run; prefer the ranges when checking for a regression.

| Scenario | Median | p99 | Throughput |
| --- | ---: | ---: | ---: |
| 1080p opaque fill | 0.145 ms | 0.165 ms | 0.07 ns/px |
| 1080p translucent fill (steady state) | 7.236 ms | 8.490 ms | 3.49 ns/px |
| 1080p opaque + translucent over it | 7.410 ms | 7.663 ms | 3.57 ns/px |
| 1080p 40 lines of text | 1.264 ms | 2.011 ms | 0.61 ns/px |
| 1080p rounded fill r=32 | 0.166 ms | 0.193 ms | 0.08 ns/px |
| 1080p half-clipped rounded fill | 0.135 ms | 0.170 ms | 0.07 ns/px |
| 4K opaque fill | 0.954 ms | 1.235 ms | 0.12 ns/px |
| 4K translucent fill (steady state) | 29.407 ms | 33.920 ms | 3.55 ns/px |
| 4K opaque + translucent over it | 30.796 ms | 38.262 ms | 3.71 ns/px |
| 4K 40 lines of text | 2.234 ms | 2.849 ms | 0.27 ns/px |
| 4K rounded fill r=32 | 1.021 ms | 1.597 ms | 0.12 ns/px |
| 4K half-clipped rounded fill | 1.136 ms | 1.806 ms | 0.14 ns/px |

## Before the rewrite

The same benchmark measured on the per-pixel path before the scanline work:

- 1080p opaque fill: 36.5-40.35 ms (19.46 ns/px)
- 4K opaque fill: 162.35 ms (19.57 ns/px)

The baseline moved between runs, so quote a range, not a single ratio:
**about 200-240x** for the 1080p opaque fill. A fast run at 0.145 ms against the
40.35 ms baseline is higher, which is why the conservative range is preferred.

## Notes

- The translucent scenario is steady state over an opaque framebuffer, which
  matches a real UI frame. A first fill over a fully transparent buffer still
  takes the float source-over path.
- Blending over an opaque destination uses an integer mul-add. It is allowed
  to differ from the old float path by at most one 8-bit step; no source PNG
  fixtures cover that case, so none were regenerated.
- Text is the dominant cost in real frames; the fill numbers above do not
  include layout or retained-tree work.
- Rounded-rect edges are still binary inside/outside. Anti-aliasing is a
  separate quality change and is not included here.
- `Image::new` and related constructors lost `const` when payloads moved to
  `Arc`. That is a breaking change and is batched for 0.2.0.
