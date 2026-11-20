# FP/SIMD performance

Measured with the `fp-bench` image (`kernel/tests/fp-bench`, `just boot-fp-bench`).
The workload is Doom's view transform and perspective projection, as in `R_AddLine`,
done in `f32` with NEON. It runs over E1M1's 467 vertices, padded to 480 (30 chunks of
16), from 64 viewpoints.

Each run is 10 000 frames: 4 800 000 vertices and 300 000 chunks. Every variant runs 5
times, alternating with the other variants. The time base is the virtual counter at
54 MHz.

## Raspberry Pi 4, without the kernel-entry FP guard

Each variant does the same projection and adds one operation per 16-vertex chunk.

| Per chunk | Fastest run | Slowest run | Added per chunk over compute only |
|---|---|---|---|
| Nothing (compute only) | 33.564 ms | 33.568 ms | 6.992 ns per vertex |
| `Notification.Signal`, no waiter | 408.232 ms | 409.121 ms | 1 248.893 ns per Signal |
| Call into an integer-only component | 1 862.228 ms | 1 865.998 ms | 6 095.548 ns per Call |
| Call into an FP component | 1 866.591 ms | 1 867.591 ms | 6 110.090 ns per Call |

- A Call into an FP component costs about 14.5 ns, or 0.24 %, more than one into an
  integer-only component. That extra is the zeroing of the callee's registers and the
  `q8..q15` and FPCR restore on Return.
- The Signal is a kernel entry and exit from an FP frame; FP state is not saved there.

The benchmark Thread's FP counters over all four variants and five runs:

| Counter | Value | Source |
|---|---|---|
| `fp_entries` | 4 501 677 | 300 000 chunks × 5 runs × 3 variants that make a call, plus the preemptions |
| `fp_saves` | 1 677 | Thread switches away from the benchmark |
| `fp_loads` | 1 677 | Thread switches back to it |
| `fp_zeroes` | 1 500 000 | Calls into the FP component |
| `preserved_saves` | 3 000 000 | Calls from the benchmark, both Call variants |
| `first_use_traps`, `decays`, `owner_hits` | 0 | not implemented |

Nucleus totals: 4 170 Thread switches, 3 351 of them with an FP frame on either side, and no
entry-guard toggles.

## Raspberry Pi 4, with the kernel-entry FP guard

Built with the nucleus feature `fp_entry_trap` (`just boot-fp-bench fp_entry_trap`). A
kernel entry from an FP frame traps FP/SIMD (a `CPACR_EL1` write and an `isb`), and the
exit into an FP frame opens it again (a `CPACR_EL1` write, synchronized by the `eret`).

| Per chunk | Fastest run | Slowest run | Added per chunk over compute only |
|---|---|---|---|
| Nothing (compute only) | 33.556 ms | 33.567 ms | 6.990 ns per vertex |
| `Notification.Signal`, no waiter | 409.504 ms | 413.342 ms | 1 253.159 ns per Signal |
| Call into an integer-only component | 1 886.096 ms | 1 891.076 ms | 6 175.132 ns per Call |
| Call into an FP component | 1 911.278 ms | 1 913.116 ms | 6 259.074 ns per Call |

The benchmark Thread's FP counters match the run without the guard: `fp_entries`
4 501 665, `fp_saves` and `fp_loads` 1 665, `fp_zeroes` 1 500 000, `preserved_saves`
3 000 000. Nucleus totals: 4 240 Thread switches, 3 327 of them with an FP frame on either
side, and 12 002 528 entry-guard toggles.

## What the guard costs

| Per chunk | Without the guard | With the guard | Difference | Guard toggles per call |
|---|---|---|---|---|
| Compute only, per vertex | 6.992 ns | 6.990 ns | none | 0 |
| `Notification.Signal` | 1 248.893 ns | 1 253.159 ns | +4.3 ns (+0.3 %) | 2 |
| Call into an integer-only component | 6 095.548 ns | 6 175.132 ns | +79.6 ns (+1.3 %) | 2 |
| Call into an FP component | 6 110.090 ns | 6 259.074 ns | +149.0 ns (+2.4 %) | 4 |

The toggles per call follow from the transitions:

- a Signal enters from and exits to the FP frame;
- a Call into an integer-only component toggles at the Call's entry and at the Return's
  exit;
- a Call into an FP component toggles at both ends of the Call and of the Return.

1.5 M calls of each kind give 12 000 000 toggles; the other 2 528 are the preemptions and
the setup. The cost per toggle is not constant: about 2 ns for the Signal, against about
40 ns for the Calls.
