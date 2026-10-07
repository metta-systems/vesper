# Invocation

| | |
|---|---|
| Wire type | `0x07` (core) |
| Target | An exported entry point in a target `AddressSpace` |
| Status | Active: Call through real SVC dispatch; created by [`AddressSpace.CreateInvocation`](../arch/address_space.md) |

## Purpose

An `Invocation` capability names one exported procedure of a component: a
target `AddressSpace` incarnation, a mandatory nonzero entry address and the
stack extent the component publishes for it. It carries only `CALL` authority
and names no server Thread — this is a Protected Procedure Call: the calling
Thread itself migrates into the target `AddressSpace`, runs the procedure, and
comes back through [`Thread.Return`](thread.md#return) on the
`CurrentReturnOnly` sentinel. Synchronous rendezvous between client and
server Threads is a userspace composition over Invocation, `Notification` and
`EventCount`, not a kernel kind (see `kernel/tests/endpoint-test`).

## User-level visible operations

| Op | Name | Wire schema | Authority | Success result |
|---|---|---|---|---|
| `0` | Call | `x2..x7` six `u64` inputs; `x9` target SP | `CALL` | Source resumes with `x0 = 0`, `x1 = r0`, `x2 = r1` — the two words the target Returns |

Every other opcode is `InvalidOperation`. Invocation has no Return opcode.

### Stack extent and SP

`CreateInvocation` publishes `[base, end)` (`x5`, `x6`) and a positive minimum
headroom `M` in bytes (`x7`). `base`, `end` and `M` are multiples of 16; the
extent lies in the target user range `[0, 1 << 48)`, is nonempty and holds `M`.
At Call the submitted SP must be 16-byte aligned with `base < SP <= end` and
`SP - base >= M`; an empty descending stack starts at `end`. These are numeric
checks only: the kernel does not walk mappings or check writability. Whether
the procedure fits in `M` is the component's concern, protected by its own
guard pages.

### Errors

| Status | Meaning | `x1` / `x2` |
|---|---|---|
| `INVALID_STACK = 32` | A stack predicate failed (construction or Call) | Offending value / reason ID below |
| `NESTING_DEPTH = 33` | The caller's depth-16 invocation stack is full | Current count (16) / `0` |
| `UNEXPECTED_RETURN = 34` | Userspace-synthesized: a Return helper saw a local `SUCCESS` | Local `x1` / `x2` |
| `InvalidPointer` | Zero entry address at construction | — |

| ID | Reason | Condition | Value |
|---:|---|---|---|
| 1 | `ExtentEmpty` | `end == base` | `end` |
| 2 | `ExtentInverted` | `end < base` | `end` |
| 3 | `BaseOutsideUserRange` | base outside the user range | `base` |
| 4 | `EndOutsideUserRange` | end beyond the user range | `end` |
| 5 | `BaseMisaligned` | base not 16-byte aligned | `base` |
| 6 | `EndMisaligned` | end not 16-byte aligned | `end` |
| 7 | `MinimumHeadroomZero` | `M == 0` | `M` |
| 8 | `MinimumHeadroomMisaligned` | `M` not 16-byte aligned | `M` |
| 9 | `MinimumHeadroomTooLarge` | `M > end - base` | `M` |
| 10 | `SpMisaligned` | SP not 16-byte aligned | `SP` |
| 11 | `SpOutOfRange` | `SP <= base` or `SP > end` | `SP` |
| 12 | `SpInsufficientHeadroom` | `SP - base < M` | `SP` |

Reason `0` and unknown IDs decode as `UnknownResponse`. Construction checks
reasons 1–9 in table order and stops at the first; Call checks 10–12 in order.

Admission order — each stage before the next, all before any state changes:

- **CreateInvocation:** operation, capabilities and authority, live target →
  nonzero entry → extent/headroom → destination slot → install.
- **Call:** operation, key, `CALL`, live target → SP → target translation
  readiness → invocation depth → commit.

Every rejection leaves the caller, its invocation stack, the destination
table and the hardware context unchanged.

### Register state across a migration

| State | Target entry (after Call) | Source resumption (after Return) |
|---|---|---|
| `x0`/`x1` | Zero (two dummy arguments) | `SUCCESS` / `r0` |
| `x2..x7` | The six inputs, unchanged | `x2 = r1`; `x3..x7` zero |
| `x8..x18` | Zero (including the consumed `x9`) | Zero |
| `x19..x30` | Zero | Exactly as the source had them |
| NZCV | Clear | The source's |
| Other SPSR bits | Inherited from the source (same EL and masks) | The source's exact SPSR |
| SP / PC | Submitted SP / entry | Source SP / after the Call SVC |

Rejected Calls and Returns are ordinary errors: nothing is scrubbed or
migrated. `Thread.Return` ignores `x4..x7`.

### Userspace convention

- `InvocationKey::call(args, target_sp)` over `libsyscall::ppc_call`, and
  `ThreadReturnKey::return_from_invocation(r0, r1) -> Result<Infallible, CapError>`
  over `ppc_return`. Both declare `x0..x18` conservatively and leave memory,
  flags and the stack unconstrained; `x18` is caller-volatile.
- `ppc_export!(entry => body)` generates the entry `CreateInvocation`
  publishes: an `extern "C"` function of eight `u64`s (two dummies, six inputs)
  that never returns normally. It calls the body with the same eight arguments
  in the same registers; the body returns a `#[repr(C)]` `PpcResult { r0, r1 }`
  in `x0`/`x1`.
- The adapter spills `(r0, r1)` to a 16-byte stack slot, Returns through the
  key recorded by `export::init_return_key`, and on a rejected Return calls
  the image-supplied `vesper_thread_return_fault(status, detail1, detail2, r0, r1) -> !`.
  It never retries or re-runs the body.

## Kernel-level implementation details

- **Capability:** `InvocationPayload` (40 B) holds the nonzero entry, the
  target `AddressSpace` identity and an immutable validated
  `InvocationStackExtent` (24 B), inside the 64 B `KeyEntry`.
- **Invocation stack:** each Thread carries a depth-16 inline array of
  kernel-private continuation records (144 B each, 2304 B total): source
  `AddressSpace`, return PC, SP, raw SPSR, exception origin, a Call time stamp
  and `x19..x30`. No runtime allocation; Thread pool backing is sized from the
  type.
- **Call:** `api::invocation::call` runs `Nucleus::prepare_call` (read-only:
  live target, `x9` from the saved frame, translation, depth) then
  `commit_call`, which re-validates, pushes the record and switches the
  Thread's `AddressSpace`, so later lookups use the target's table. The SVC
  entry installs the target translation after guards and the kernel lock are
  released, restores the scrubbed frame and traces `✅ Invocation::Call()`.
  Everything runs on the per-core kernel stack; no continuation lives there.
- **Blocking while migrated:** a Thread inside a target may wait on a
  `Notification` or `EventCount`; its wait continuation is a separate
  Thread-resident slot, and resumption reinstalls the target it is in.
- **Validation:** `kernel/tests/kicktest` (same-Thread round trips into Bounce,
  register and scrub checks, rejections, compiled exports) and
  `kernel/tests/endpoint-test` (three `AddressSpace`s, Threads blocking inside
  the endpoint); admission priorities in the `debug_console` unit tests.

## Sidenotes

- The `x9` SP transport and the two-word body-return convention are
  provisional; freezing either is a maintainer decision.
- The entry address is stored as supplied; the kernel validates neither its
  mapping nor that it is executable.
- Execution so far is trusted `EL1t`; nothing here is a hostile-EL0
  confinement claim.
- `libobject::export` keeps one Return key per image, so a test image hosts
  one PPC-target component.

## TODOs

- Return fault delivery (underflow, retired source) — D1/D7; currently an
  interim kernel panic with nothing popped.
- EL0 execution of components.
- Effective FP/SIMD trapping for the integer-only slice; TLS, debug and other
  architectural-state isolation.
- Invocation distribution: CopyDerive restrictions, rights attenuation,
  badges — D4.
- Pointer and shared-memory arguments; capability transfer.
- Cancellation and teardown of a Thread blocked inside a migrated call;
  nested and concurrent-call rules beyond the depth bound.
- Per-call time attribution on Return (needs the Time subsystem).
