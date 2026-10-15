# Future: pointer-based `libmmio` with tock-registers ergonomics

Status: **shelved.** The drivers use [derive-mmio](https://docs.rs/derive-mmio) with
[bitbybit](https://docs.rs/bitbybit) bitfields. Come back to this design if that combination
proves too verbose or too limited. `libs/platform/src/raspberrypi/device_driver/bcm/gpio_libmmio.rs`
(a sketch that is not compiled) shows the BCM GPIO driver written against this design;
`gpio.rs` next to it is the same driver on derive-mmio.

## What the derive-mmio port showed

Points to weigh when revisiting this design, from porting every driver to derive-mmio 0.8:

- **Array accessors form a reference.** derive-mmio 0.8 reaches an array element through
  `(*self.ptr).field.as_mut_ptr()`, which auto-references the whole array. Single registers use
  `addr_of!` and form no reference. Worth an upstream fix.
- **Plain volatile accesses.** Reads and writes are `read_volatile` / `write_volatile`, with no
  single-instruction guarantee for trap-and-emulate under a hypervisor.
- **No offset checks.** derive-mmio checks only that the fields leave no padding. Every block
  carries `libmmio::assert_register_offsets!` for the datasheet offsets; it caught a wrong gap
  length during the port.
- **`&mut` for writes meets `&self` traits.** `libconsole::SerialOps` takes `&self`, so the
  platform UARTs make a short-lived handle with the unsafe `clone()` under their lock.
- **Handle names are always public.** The generated `Mmio<Name>` is `pub` whatever the block's
  visibility, so blocks re-exported through one glob need distinct names.
- **Arrays return `Result`.** Every array access returns `Result<_, OutOfBoundsError>`, even for
  indices that are in range by construction.
- **Fixed field access.** A register's access is fixed per field, so a register whose read and
  write are separate hardware paths (the PL011 data register) cannot be split between two
  owners. Two Threads each make their own handle instead.

## Problem

tock-registers 0.10 register blocks are reached through `&RegisterBlock`, either directly
(`&*(address as *const RegisterBlock)`) or through a `Deref` wrapper, which cannot
uphold its safety comment. A reference to MMIO is `dereferenceable`, so LLVM may insert reads the
code never asked for. That is undefined behaviour, and on a FIFO or a read-to-clear status register
it has real side effects.

safe-mmio avoids this with raw pointers and splits reads into pure reads (`ReadPure`) and
side-effecting reads (`ReadOnly`). It has no bitfield DSL. The goal is the safety of safe-mmio with
the `register_bitfields!` / `read(FIELD)` / `modify(A + B)` expressiveness of tock-registers.

## Survey (2026-10)

| Crate | No references | Pure vs side-effect read | tock `register_bitfields!` | Notes |
|---|---|---|---|---|
| tock-registers 0.10.1 | no (`&RegisterBlock`, `UnsafeCell` fields) | no | yes | the problem |
| tock-registers `main` redesign (`mmio32_register_map!`, `Real<B: Bus>`) | yes (wraps a pointer) | no: register handles are `Copy` and take `self` by value, so nothing like `&mut` gates a FIFO pop | yes | unreleased (git only), API in flux |
| safe-mmio 0.3 | yes | yes (`ReadPure`, `ReadOnly`, `ReadPureWrite`…) | no: `zerocopy` integers or bitflags | `UniqueMmioPointer::read(&mut self)` cannot back tock's `Readable::get(&self)`; its aarch64 asm backend is private |
| derive-mmio 0.8 | yes | yes (`PureRead` takes `&self`, `Read` takes `&mut self`) | no: bitbybit bitfields, accessors `read_x` / `modify_x(closure)` | the closest; the choice for now |
| svd2rust / chiptool | chiptool yes | no | no | SVD/YAML code generation, which does not suit handwritten blocks |

## Key insight

`tock_registers::interfaces::{Readable, Writeable}` require only `get(&self) -> T` and
`set(&self, T)`. Every ergonomic method is a default method: `read(field)`, `read_as_enum`, `is_set`,
`matches_all`, `matches_any`, `extract`, `write(FieldValue)`, `modify_no_read`, `debug()`.
`ReadWriteable::modify` is a blanket impl. So implementing `Readable` / `Writeable` for a **handle
that holds a raw pointer** gives the whole bitfield API unchanged. `register_bitfields!`, `Field`,
`FieldValue`, `LocalRegisterCopy` and `TryFromValue` do not depend on the register types. The same
traits back the `aarch64-cpu` system registers, so MMIO and system-register code keep one vocabulary.

## Design (`libs/mmio`)

### Access cells

The access cells are inert `#[repr(transparent)]` cells over `T: UIntLike`, carrying `PhantomData<R>`.
They have no methods and no public constructor, so they can only be named in a layout:

| Cell | Read | Write |
|---|---|---|
| `ReadPure<T, R>` | pure | — |
| `ReadOnly<T, R>` | side effects | — |
| `WriteOnly<T, R>` | — | yes |
| `ReadPureWrite<T, R>` | pure | yes |
| `ReadWrite<T, R>` | side effects | yes |
| `Aliased<T, Rread, Rwrite>` | side effects | yes, on a separate hardware path (PL011 DR: RX FIFO pop, TX FIFO push) |

A sealed `Access` trait maps each cell to its capabilities and to the handle each receiver produces.

### Register handles

`Register<'a, T, R, Mode>` holds `NonNull<T>` and `PhantomData<(&'a (), R, Mode)>`. `Mode` is one of:

- `Pure`: obtained from `&block`. It is `Copy`, `Send` and `Sync`, and implements `Readable` only.
- `Exclusive`: obtained from `&mut block`. It is not `Clone` and is `!Sync` (through
  `PhantomData<Cell<()>>`). It implements `Readable` and `Writeable` as the cell allows. Because the
  handle cannot be duplicated or shared across threads, `get(&self)` / `set(&self)` carry the same
  guarantee as safe-mmio's `&mut` methods.

`get` / `set` go through a backend:

- aarch64: single-instruction `ldr{b,h}` / `str{b,h}` inline asm, as in safe-mmio. `read_volatile`
  may compile to writeback addressing forms. Those report `ISV=0`, so a hypervisor (KVM, HVF) cannot
  emulate them.
- other targets and host tests: `read_volatile` / `write_volatile`.

`RegisterArray<'a, T, R, Mode, N>` provides `get(index) -> Option<Register>`, `iter()` / `iter_mut()`
and `len()`.

### Block handles

`Mmio<'a, B>` is unique, `!Sync` and not `Clone`. `SharedMmio<'a, B>` is `Copy` and `Sync` and allows
pure reads only. Both hold `NonNull<B>`.

- `unsafe fn Mmio::new(ptr: NonNull<B>)`. Safety: `ptr` is a device mapping of `size_of::<B>()`,
  valid for `'a`, with no other handle to it.
- `fn shared(&self) -> SharedMmio<'_, B>` and `fn reborrow(&mut self) -> Mmio<'_, B>`.
- Field projection uses `&raw mut (*ptr).field` only. No reference to device memory is ever formed.

### `register_block!`

It takes the rows of `register_structs!` plus a handle name:

```rust
register_block! {
    /// The PL011 register block.
    pub RegisterBlock => Registers {
        (0x000 => data: Aliased<u32, DR::Register, DR::Register>),
        (0x018 => flag: ReadPure<u32, FR::Register>),
        (0x030 => control: ReadPureWrite<u32, CR::Register>),
        (0x040 => masked_interrupt_status: ReadPure<u32, INT::Register>),
        (0x044 => interrupt_clear: WriteOnly<u32, INT::Register>),
        (0x1000 => @END),
    }
}
```

It expands to:

- The `#[repr(C)]` layout, produced by `tock_registers::register_structs!` over the inert cells. That
  reuses its padding and offset assertions.
- `pub struct Registers<'a>(Mmio<'a, RegisterBlock>)` with `unsafe fn new`, `shared()` and one
  accessor per row. The macro picks the receiver by matching the cell ident:
  - `name(&self)` exists when reads are pure and returns a `Pure` handle;
  - `name_mut(&mut self)` exists when the register is writable or its reads have side effects, and
    returns an `Exclusive` handle;
  - `Aliased` also gets `name_split(&mut self) -> (reader, writer)`.
- A `split!` macro hands out disjoint `Exclusive` field handles to separate owners, like safe-mmio's
  `split_fields!`, with each field name checked once at compile time.

`name_mut` is built with `${concat(name, _mut)}`, which needs the nightly feature
`macro_metavar_expr_concat`. The `paste` crate is the fallback. Register names become snake_case.

Driver code keeps its shape:

```rust
registers.control_mut().modify(CR::UARTEN::Enabled);
registers.flag().is_set(FR::BUSY)
```

Holding one `Exclusive` handle borrows the whole block mutably. Code that needs several registers at
once uses `split!`.

### PL011 as the first consumer

- `Pl011<'registers>` holds `Registers<'registers>`. Configuration takes `&mut self`.
- The two Threads get disjoint handles from `split!`. The interrupt Thread gets the `data` reader
  half, `masked_interrupt_status` and `interrupt_clear`. The transmit Thread gets the `data` writer
  half and `flag`. The ownership rule now in the driver's module comment becomes a type-checked split.
- Host tests build the handle over leaked memory with `Registers::new(NonNull)`.

## Validation for an implementation

1. Host unit tests over a plain `[u64]` buffer:
   - `read`, `write`, `modify` and `read_as_enum` round-trip;
   - array bounds return `None`;
   - layout offsets match the rows;
   - `Aliased` halves address the same word.
2. Compile-fail tests (`trybuild` or `compile_fail` doctests):
   - `name_mut()` through `SharedMmio` is rejected;
   - a side-effecting read without `&mut` is rejected;
   - sharing an `Exclusive` handle across threads by reference is rejected;
   - passing the same field twice to `split!` is rejected;
   - writing a `ReadPure` register is rejected.
3. Disassembly: device accesses compile to plain `ldr` / `str` with no writeback forms.
4. `just clippy`, `just fmt-check`, `just test-host` and the QEMU driver tests (`just test-uart`,
   `just test-uart-rpi4`).
