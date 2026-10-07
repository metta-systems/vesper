# Time

| | |
|---|---|
| Wire type | `0x04` (core) |
| Pool | none — no active storage |
| Status | Excluded sketch: no handler, no object, dispatch returns `UnsupportedCoreType` |

## Purpose

A `Time` capability is intended to represent authority over a bounded CPU
budget — a first-class, delegatable resource distinct from memory. The kernel
enforces budget consumption; userspace schedulers implement policy,
distribute budget hierarchically, and observe DCBs. Time is the mechanism
behind user-level scheduling, donation-based IPC accounting, and
mixed-criticality/real-time operation.

## User-level visible operations

None active. The intended vocabulary (contract qualification only — no
operation is supported end-to-end):

| Op | Name | Intended semantics | Status |
|---|---|---|---|
| `0` | Donate | Authorize execution of a target using a defined budget; donor suspends until yield/exhaustion/cancellation | Deferred (D8) |
| `1` | Split | Create a child budget in an explicit destination, reducing the parent's remaining amount only on commit | Deferred (D8) |
| `2` | Merge | Combine compatible budgets without double counting; check parent/provenance and deadline compatibility | Deferred (D8) |
| `3` | Query | Observe remaining budget through the checked result convention | Deferred (D8) |

Invoking a Time capability today returns `UnsupportedCoreType` from core
dispatch (`kernel/nucleus/src/api/mod.rs`).

## Kernel-level implementation details

- No kernel object exists. `kernel/nucleus/src/objects/time.rs` holds an
  excluded sketch (`remaining_us`, `deadline`, `parent` for a custom
  revocation tree) that does not compile as-is; `api/time.rs` holds an
  excluded handler sketch (Donate/Split/Merge/Query against a `TimeKey`).
  Module inclusion and dispatch determine reachability — these files are not
  wired into dispatch.
- Budget conservation is a hard contract for every future operation: memory
  cannot mint positive budget; root budget issuance/replenishment requires
  explicit scheduler authority; multiprocessor ownership and simultaneous
  donation must not permit spending the same budget twice.
- Unit mismatch to resolve (D8): the sketches use microseconds; DCB time
  accounting uses nanoseconds. Nanosecond internal accounting is the
  recommendation, not an announced ABI change.
- The completion foundation (`ExecutionContext`, pending-invocation records)
  and `DeactivateReason::TimeExhausted` in `Nucleus` record where donation and
  expiry will attach, but nothing is enabled.

## Sidenotes

- Time is the clearest "mechanisms, not policies" object in the catalogue:
  the kernel provides enforcement and accounting primitives; the scheduler is
  a userspace service.
- IPC-related time donation must obey the same accounting contract
  (migrating-threads attribution — work performed by servers on behalf of a
  client is attributed to the client).

## TODOs

- Everything: budget issuance, donation loan-vs-transfer semantics,
  unused-budget return to parent, split/merge/deletion/expiry, units/clocks,
  rounding/overflow, multicore accounting — D8 (Time/scheduler vertical
  slice, Phase 7).
- Settle whether donation is a temporary loan or permanent transfer, and
  where the remainder resides, before exposing consuming Rust wrappers.
