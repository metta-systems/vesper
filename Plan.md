- [ ] Rename Kickstart to Ymir?
- [ ] VSpace -> Container?
  - [ ] Component?
- [x] CSpace = KeyTable
- [-] our Domain shall be more like Protection Domain (= TCB + CSpace + VSpace), 
- [ ] ObjectPool .meta is limited to 256 entires, which is wrong - the meta should be allocated together with the pool from the Untyped, this is where we know the actual capacity.
- [ ] seemingly AI decided we use process stacks, while it should be interrupt stacks for kernel state. Domain context should be stored in the DCB.
- [x] Extract testing bits from kickstart into a separate kicktest binary. (2026-09-24: kickstart is now a lib + real-boot bin — the e2e suite, Bounce fixture, and test helpers live in the separate kernel/kicktest crate reusing the shared boot path; `just test-capability-boot` builds/boots the kicktest image via `just build-kicktest`)
- [ ] pub asid: Option<u16>, should probably use Option<NonZero<u16>> in domain.rs
  - ASID 0 reserved for the kernel's own boot translation context
  - [ ] similarly for some other int-based optionals, where 0 is clearly an invalid value - find them

- [ ] "a per-domain IPC buffer receives anything beyond register capacity. The buffer's registered location and lifetime are part of domain creation (D5/D6 follow-up)." - does a per-Endpoint IPC buffer make more sense? How it is implemented?

- [ ] Make KeySlot::DEBUG_CONSOLE not be slot 127, install it somewhere else (the capability will be 127 though, so it's more natural)

- [ ] two mmio tests ignored -- running 2 tests
  test libs/mmio/src/lib.rs - MMIODerefWrapper<T> (line 42) ... ignored
  test libs/mmio/src/lib.rs - MMIODerefWrapper<T> (line 46) ... ignored

- [ ] "cannot parse the debug map for" - where it is from, toolchain or my scripts?
    info: Vector table:        0xFFFF800000003800 (size: 0x800)
  error: cannot parse the debug map for '/Users/berkus/Projects/Metta/vesper/target/aarch64-metta-none-eabi/release/build/kickstart/05af02c1fa98982b/out/kickstart': The file was not recognized as a valid object file

- [/] What's needed: quick-n-dirty higher-half mappings setup and kernel physical memory view setup (both in TTBR1), identity mapping for kickstart (in TTBR0).
  - `__kernel_start` to `__kernel_end` map at KERNEL_HIGH_BASE
  - 0 to phys_ram_size (from DTB) map at KERNEL_PHYS_WINDOW
  - `__kickstart_start` till `__kickstart_end` identity-map (no code/data split yet?)

Steps:
- [ ] Move tests from kernel/tests to individual libs?

- [x] Make some caps work - Untypeds, Domains, Buffers, what else?
- [x] Test out syscalls from EL0

- START FILLING IN CAPS
  - [ ] untypeds
  - [ ] kickstart context and domain

---

# Docs

What was created

`vesper/doc/object_types/` now contains a complete reference for every kind in the capability catalogue, grounded in the current implementation (`kernel/nucleus/src/objects/`, `kernel/nucleus/src/api/`, `libs/object/src/`) and the three contract docs:

- **`README.md`** — index, wire encoding, invocation model, and a dispatch diagram
- **`core/`** — `null`, `untyped`, `domain`, `key_table`, `time`, `endpoint`, `notification`, `event_count`, `reply`, `debug_console`
- **`arch/`** — `frame`, `page_table`, `vspace`, `asid_pool`, `asid`, `io_space`, `io_port`, `irq_handler`, `irq_control`

Each document follows the same structure: name/metadata table → purpose → user-level visible operations (wire schema, authority, results) → kernel-level implementation details (with small mermaid diagrams: retype transaction, retire teardown, KeyTable lookup precedence, Notification state machine, EventCount overflow flow, Frame.Map transaction, PageTable installation, ASIDPool.Assign, Endpoint rendezvous, dispatch) → sidenotes → TODOs → **final cross-reference section** against the 🧠 Vesper vault.

## Most important discrepancies flagged for your review

1. **KeyTable architecture** (`key_table.md`): vault describes a KeyNode directed graph with guarded page tables, radix/depth-limits, kernel CDT, and recursive revoke; current implementation is a flat 256-slot table, no CDT, Revoke rejected. Also 32-byte `KeyEntry` vs the vault's assumed 16 bytes/slot.
2. **Single address space** (`domain.md`, `vspace.md`, `frame.md`): vault SAS vision vs. selected per-Domain translation contexts (D1).
3. **Untyped split + device retyping** (`untyped.md`): vault expects Untyped→Untyped splitting and device-Untyped→Frame retyping; both are rejected today.
4. **ASID pools** (`asid_pool.md`): vault/seL4 imply Retype-creatable pools; current model is boot-provided-only.
5. **IRQ delivery** (`irq_handler.md`, `irq_control.md`): the entire interrupt HAL is an unchecked vault todo; Notification is ready as the target but nothing delivers to it.
6. **IPC surface** (`endpoint.md`): vault expects non-blocking Send/Recv variants and a `Yield` syscall — neither exists nor is contracted.

## Questions I skipped (per your instruction)

1. **Scope of "currently existing"** — I documented *all* catalogue kinds (including reserved/deferred ones like `VSpace`, `ASID`, `IOPort`) with explicit status lines, rather than only the active kinds. Trim if you wanted active-only.
2. **README index** — I added one; remove it if you want only the per-type files.
3. **Untyped→Untyped split** — treated as a TODO/discrepancy rather than a deliberate rejection; confirm intent.
4. **Vault phrasing "kernel creates Reply object"** — flagged in `reply.md` as a mechanism divergence (pending-record vs pooled object); needs your call.

No code was changed, so no build/test validation was run; the mermaid diagrams use only supported types (flowchart, stateDiagram, sequenceDiagram).


---

# Completed

- [x] Buildable
- [x] build all shit together into a binary
- [x] make separate kickstart section and start booting from kickstart
- [x] make early_print work
- [x] parse dtb
- [x] Print that we can invoke kernel function using a syscall from the kickstart (even it if runs at the same EL for now)
- [x] Enter kernel init in EL2 - this will be needed to set up kernel mappings
- [x] Print DTB
- [x] Print max RAM from DTB

- [x] Allocate a single key slot in a global domain struct
- [x] Fill it with capability to DebugConsole
- [x] Invoke DebugConsole.Write via syscall

- [x] Generate complete memory map from DTB, print it out

- [x] Print kernel covered area
- [x] Print KERNEL_HIGH_BASE
- [x] Print kernel mappings size and attribs
- [x] Print kickstart covered area
- [x] Print kickstart mappings size



Whatever kernel links must also be located in high-mem mapping, so we cannot share this code with kickstart at all!
This means it's probably sensible to build kernel as a separate ELF file linked entirely high, then merge it with the kickstart binary through specially-named sections; there should be no symbol resolution across two binaries, so the nucleus image is solely pulled via it's PHDRS (but we need to place the BSS which will be erased by the kickstart before turning the MMU on)

- [x] See gh:metta-systems/kernel-embed-prototype for an outline of this approach - copy it here and lets go.
