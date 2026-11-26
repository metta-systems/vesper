# Logging open decisions

Parking lot for unanswered logging decisions. Each entry stays open until the maintainer
answers it explicitly. Answered decisions go into
[logging-design.md](logging-design.md#decisions); their tasks into
[logging-implementation-plan.md](logging-implementation-plan.md).

## L13. DebugConsole byte transport

Parked. How does a generally available console receive its bytes?

- **(A)** Keep the pointer-based Write: the nucleus needs checked user-memory reads
  (caller-AddressSpace authorization, input snapshot, copy-fault recovery), which no other
  operation currently needs.
- **(B)** A register-inline operation, e.g. a length plus up to 40 bytes in `x3..x7` with
  client chunking, so the nucleus never reads user memory.

Until this is decided the console stays a debug-only prototype. The hardware debugging
aid's checked pointer reads ([DebugConsole debug-only
exception](logging-design.md#debugconsole-debug-only-exception)) are temporary and do not
answer this question. Capability decisions D1, D6 and D9 touch it.
