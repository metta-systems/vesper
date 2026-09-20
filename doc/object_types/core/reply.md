# Reply

| | |
|---|---|
| Wire type | `0x08` (core) |
| Pool | none — no active storage |
| Status | Excluded sketch: no handler, no object, dispatch returns `UnsupportedCoreType` |

## Purpose

A `Reply` capability is intended to be **one-shot reply authority**: created
by the kernel for a particular pending `Call` invocation, handed to the
receiver, and consumed exactly once when the reply is delivered to the
original caller. Explicit reply capabilities (rather than an implicit
server-side reply slot) enable delayed replies, authorized reply delegation,
and precise cancellation semantics — the seL4-MCS direction.

## User-level visible operations

None active. The intended vocabulary (contract qualification only):

| Op | Name | Intended semantics | Status |
|---|---|---|---|
| `0` | Send | Deliver the reply message to the blocked caller; consumes the Reply | Deferred (D7) |
| `1` | SendWithCap | As Send, plus one transferred capability | Deferred (D7) |
| `2` | SendError | Complete the caller with an error instead of a reply | Deferred (D7) |

Invoking a Reply capability today returns `UnsupportedCoreType`.

## Kernel-level implementation details

- No kernel object exists. `kernel/nucleus/src/objects/reply.rs` holds an
  excluded sketch (caller + `Pending`/`Used`/`Cancelled` state machine) that
  does not compile as-is; `kernel/nucleus/src/api/reply.rs` holds an excluded
  handler sketch. Neither is wired into dispatch.
- The selected model any implementation must follow (2026-09-16):
  - A Call produces one-shot Reply authority associated with a particular
    **pending-invocation record** carrying a non-wrapping 64-bit generation
    (closed-wait identity) — never a bare slot number or domain index.
  - A reply is consumed exactly once at successful commit, or retired by
    explicit cancellation/teardown. Pre-commit errors retain ownership.
  - **Late replies**: a reply arriving after the caller timed out or was torn
    down consumes/retires the one-shot Reply and returns a defined error to
    the replier (exact shared status is D9 work). Nothing is retained for a
    caller that already returned.
  - Reply authority cannot be copied into independently usable replies.
  - Resource reservation for reply records must be bounded and accounted.
- The terminal-transition rule: reply, timeout, cancellation, and teardown
  compete for exactly one terminal transition of the pending record; losers
  observe a defined state.
- Kind history: renumbered 9 → 8 when `Buffer` was removed from the catalogue
  (2026-09-15). A wire value 8 decodes as `Reply`; there is no Buffer kind.

## Sidenotes

- The excluded sketch's `ReplyAlreadyUsed` error and kernel-allocated reply
  pool are superseded: reply state belongs to pending-invocation records, and
  errors must come from the shared status space (D9), not a per-family wire
  error space.
- ReplyRecv (Endpoint op 3) composes Reply.Send + Recv atomically and must
  distinguish reply-committed/receive-failed from failure-before-replying.

## TODOs

- Exact transfer/cancellation encoding, reply destination and failure
  semantics, per-operation register assignments — D7.
- Shared wire encoding for the late-reply error — D9.
- Resolve the conflicting legacy Endpoint `Reply` op 4 versus this kind's
  `Send` family — D7.

## Cross-reference: implementation vs. desired capabilities (🧠 Vesper vault)

- `IPC and PPC/IPC and PPC.md` (vault): "Reply — send to a **one-off reply
  capability**" and "Reply-then-Recv — send reply atomically followed by a
  recv" — **consistent in intent**: the selected one-shot, explicit-reply
  model matches the vault's MCS-style direction ("explicit reply
  capabilities … making invocation more like a function call"). Nothing is
  implemented yet.
- Vault: "kernel creates Reply object on Call arrival" (sketch diagram) —
  **mechanism divergence to note**: the selected model stores reply state in
  the kernel pending-invocation record rather than a separately allocated
  reply *object*; the Reply capability will authorize the record's terminal
  transition. Review whether the vault phrasing implies a pooled object the
  current contract deliberately avoids.
- `Design Requirements.md` (vault): migrating-threads attribution — reply
  handling is where donation-based accounting will attach; unimplemented.
