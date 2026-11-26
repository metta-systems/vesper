/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

//! Vesper single-address-space nanokernel.
//!
//! This crate implements the kernel binary proper.

#![no_std]
#![no_main]
#![feature(decl_macro)]
#![feature(format_args_nl)]
#![feature(likely_unlikely)]
#![feature(stmt_expr_attributes)]
#![feature(slice_ptr_get)]
#![deny(missing_docs)]
#![deny(warnings)]
#![allow(unused)]
#![allow(internal_features)]
#![allow(linker_messages)]
#![feature(core_intrinsics)]

use {
    cfg_if::cfg_if,
    core::{
        arch::asm,
        panic::PanicInfo,
        sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering},
        time::Duration,
    },
    libcpu::endless_sleep,
    libexception::arch::aarch64::{ExceptionContext, ExceptionOrigin, SavedContext},
    liblocking::TicketLock,
    liblog::{info, println, warn},
    libmapping::AccessPermissions,
    libobject::{
        ArchType, CapError, KeySlot, RawKey,
        fault::{FaultInfo, FaultKind},
        syscall_status,
    },
    libqemu::semihosting as semi,
    libstacktrace::{Backtrace, current_frame_pointer, read_own},
    nucleus::{
        board_console,
        objects::{
            ArchObjects, ArchObjectsImpl, Nucleus,
            access::{Access, ObjectId},
            arch::fp_unit::FpEntryGuard,
            completion::PendingKind,
            fault::FaultDelivery,
            resume::{PreparedResume, PreparedTranslationContext, Switch},
        },
        user_memory,
    },
};

// TODO: Split this into read-only part, that does not need locks, per-cpu mutable part that does not need locks,
// TODO: Shared atomic counters that do not need locks and shared mutable collections that DO need locks (but should be minority)

/// The Great Kernel Lock (SMP C2(c)). The boot-carved [`Nucleus`] is accessed under it.
static KERNEL_LOCK: KernelLock = KernelLock(TicketLock::new());

/// The one kernel lock: a ticket spinlock every kernel entry from a Thread takes on
/// arrival ([`KernelLock::enter`]) and releases when it returns, held by the core's
/// logical index plus one. The idle loop releases it around `wfi`, and the interrupt
/// that ends the wait takes it again as an entry of its own.
///
/// Per-core state outside the [`Nucleus`] (the core's record atomics) is not under it.
/// A core holding it never waits for another core: the only wait is taking it, and a
/// core that already holds it panics there instead of waiting for itself.
struct KernelLock(TicketLock);

impl KernelLock {
    /// Take the lock for this kernel entry, until the returned guard drops. A
    /// core still waiting when another core panics parks instead: the panicking
    /// core keeps the lock.
    fn enter(&'static self) -> KernelEntry {
        self.take(this_core_holder());
        KernelEntry
    }

    /// Take the lock for `holder`, or park if another core panics meanwhile.
    fn take(&self, holder: u32) {
        if !self.0.acquire_while(holder, || !another_core_panicked()) {
            park_for_panic()
        }
    }

    /// Run `section` on the Nucleus state the lock guards.
    ///
    /// # Panics
    ///
    /// If this core does not hold the lock: every caller runs inside a kernel entry.
    fn held<R>(&self, section: impl FnOnce(&mut ()) -> R) -> R {
        assert!(
            self.0.is_held_by(this_core_holder()),
            "kernel state touched without the kernel lock"
        );
        section(&mut ())
    }

    /// Let other cores in while this core waits for an interrupt, and take the lock
    /// again after.
    fn release_around(&self, wait: impl FnOnce()) {
        let holder = this_core_holder();
        self.0.release(holder);
        wait();
        // Another core's panic stops this one here, holding nothing.
        if another_core_panicked() {
            park_for_panic()
        }
        self.take(holder);
    }
}

/// `smp-test` only: the logical CPU whose ping SVC sent the last `IPI_PING`,
/// which the pinged core answers with `IPI_PONG`.
#[cfg(feature = "smp_test")]
static PINGING_CORE: AtomicU32 = AtomicU32::new(0);

/// Send IPI `kind` (a `core_record::IPI_*` bit) to every logical CPU of
/// `cpu_mask`: set the kind in each core's record, then raise their IPI line.
/// Whether the controller sent it.
fn send_ipis(cpu_mask: u32, kind: u32) -> bool {
    let mut targets = 0_u32;
    for cpu in 0..u32::BITS {
        if cpu_mask & (1 << cpu) == 0 {
            continue;
        }
        if let Some(record) = nucleus::core_record::of(cpu as usize) {
            record.pending_ipis.fetch_or(kind, Ordering::Release);
            targets |= 1 << cpu;
        }
    }
    targets != 0 && nucleus::platform::send_ipi(targets)
}

/// Take the IPI kinds sent to this core, after claiming its IPI line: a halt
/// parks the core here; whether a reschedule was asked for. Stop's work
/// comes with synchronous retirement.
fn take_ipis() -> bool {
    let record = nucleus::core_record::current();
    record.ipis_taken.fetch_add(1, Ordering::Relaxed);
    let kinds = record.pending_ipis.swap(0, Ordering::Acquire);
    if kinds & nucleus::core_record::IPI_HALT != 0 {
        park_for_panic()
    }
    #[cfg(feature = "smp_test")]
    if kinds & nucleus::core_record::IPI_PING != 0 {
        let pinging = PINGING_CORE.load(Ordering::Relaxed);
        send_ipis(1 << pinging, nucleus::core_record::IPI_PONG);
    }
    kinds & nucleus::core_record::IPI_RESCHEDULE != 0
}

/// The kernel-lock holder id of the core that panicked first, or 0. The other cores
/// park when they next look: in their idle loops, until the Halt IPI (SMP3) stops them
/// wherever they are.
static PANICKED_CORE: AtomicU32 = AtomicU32::new(0);

/// Whether a core other than this one has panicked.
fn another_core_panicked() -> bool {
    let panicked = PANICKED_CORE.load(Ordering::Acquire);
    panicked != 0 && panicked != this_core_holder()
}

/// Stop this core for another core's panic: interrupts masked, touching nothing shared.
fn park_for_panic() -> ! {
    libexception::asynchronous::local_irq_mask();
    loop {
        aarch64_cpu::asm::wfe();
    }
}

/// How long the panicking core waits for the kernel lock before it prints anyway.
const PANIC_LOCK_WAIT_MS: u64 = 1000;

/// Claim the panic for this core: the first core to panic prints, under the kernel lock,
/// and wakes the others to park; a core that panics after another parks at once,
/// printing nothing. The panicking core waits at most [`PANIC_LOCK_WAIT_MS`] for the
/// lock: the panic may be in the lock itself, or the holder may never let it go.
fn claim_panic() {
    // `u32::MAX` names a core without a record, which nothing else uses.
    let holder = nucleus::core_record::try_current().map_or(u32::MAX, |record| record.index + 1);
    match PANICKED_CORE.compare_exchange(0, holder, Ordering::AcqRel, Ordering::Acquire) {
        Ok(_) => {}
        // This core panicked again while panicking: `libmachine` ends that.
        Err(first) if first == holder => return,
        Err(_) => park_for_panic(),
    }
    // A core still waiting on `wfe` for the controller looks at the flag.
    libcpu::send_event();
    if holder == u32::MAX {
        return;
    }
    if !KERNEL_LOCK.0.is_held_by(holder) {
        let ticks = nucleus::timer::frequency() * PANIC_LOCK_WAIT_MS / 1000;
        let start = nucleus::timer::counter();
        while !KERNEL_LOCK.0.try_acquire(holder)
            && nucleus::timer::counter().saturating_sub(start) <= ticks
        {
            core::hint::spin_loop();
        }
    }
    // Every other core that takes interrupts parks on its Halt IPI, wherever it is.
    if let Some(record) = nucleus::core_record::try_current() {
        let all = (1_u32 << record.core_count) - 1;
        send_ipis(all & !(1 << record.index), nucleus::core_record::IPI_HALT);
    }
}

/// The kernel lock held for one kernel entry; dropping it releases the lock.
struct KernelEntry;

impl Drop for KernelEntry {
    fn drop(&mut self) {
        KERNEL_LOCK.0.release(this_core_holder());
    }
}

/// This core's kernel-lock holder id: its logical index plus one.
fn this_core_holder() -> u32 {
    nucleus::core_record::current().index + 1
}

/// Anchor to the boot-carved [`Nucleus`].
///
/// Written once by Kickstart (the one-time boot code) before the nucleus runs;
/// the inert nucleus only reads it. Kept as an atomic pointer so boot code can
/// record it via [`nucleus_set_anchor`] without the compiler constant-folding
/// the read (the static is only ever written from boot code).
static NUCLEUS: AtomicUsize = AtomicUsize::new(0);

/// Record the boot-carved [`Nucleus`] address.
///
/// Called once by Kickstart before the nucleus runs; the inert nucleus performs
/// no other initialization. Exported so the compiler cannot constant-fold the
/// anchor read (the static is only ever written from boot code).
///
/// # Safety
/// Must be called exactly once, before any syscall, from the single boot core
/// with interrupts masked. `ptr` must point at a live, exclusively-owned
/// boot-carved [`Nucleus`].
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.bootstrap")]
pub unsafe extern "C" fn nucleus_set_anchor(ptr: *mut Nucleus<nucleus::objects::ArchObjectsImpl>) {
    // SAFETY: one-shot boot write, single-core, before any syscall.
    unsafe {
        NUCLEUS.store(ptr as usize, Ordering::Relaxed);
    }
}

/// Install the privileged interrupt-controller component kickstart loaded,
/// and return its physical timer line, or `u32::MAX` if it was refused.
///
/// # Safety
/// Boot-only, once, from the single boot core with interrupts masked, after
/// kickstart mapped the component and its MMIO as `handoff` describes.
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.bootstrap")]
pub unsafe extern "C" fn nucleus_set_platform(handoff: *const libirqchip::PlatformHandoff) -> u32 {
    // SAFETY: the caller's contract.
    match unsafe { nucleus::platform::install(&*handoff) } {
        Ok(line) => {
            // The nucleus owns the timer from now on. It stays off until the
            // first kernel exit finds something that needs it; it is taken
            // whenever a Thread runs with interrupts unmasked.
            nucleus::platform::unmask(line);
            let record = nucleus::core_record::current();
            if !nucleus::platform::init_cpu(record.index) {
                warn!("Interrupt controller refused core {}", record.index);
            }
            // The secondary cores wait on `wfe` for the component to appear.
            libcpu::send_event();
            line
        }
        Err(error) => {
            warn!("Interrupt controller refused: {error:?}");
            u32::MAX
        }
    }
}

/// Use the console UART whose registers Kickstart mapped at `base` in the
/// kernel half as the board console (`nucleus::board_console`).
///
/// # Safety
/// Boot-only, from the single boot core with interrupts masked. `base` is the
/// kernel-half address of a PL011's registers, mapped as device memory for as
/// long as the nucleus runs.
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.bootstrap")]
pub unsafe extern "C" fn nucleus_set_board_console(base: usize) {
    // SAFETY: the caller's contract.
    unsafe {
        board_console::attach(base);
    }
}

/// Where a secondary core waits while it runs no Thread (SMP1). Kickstart's secondary
/// entry jumps here at `EL1h` on the core's trap stack, interrupts masked, MMU and caches
/// on, once the core is online.
///
/// Until the boot core installs the interrupt controller, the core waits on `wfe`
/// (the boot core's `sev` after the install, or a panicking core's, wakes it). It then
/// prepares itself at the controller (`init_cpu`, under the kernel lock) and waits on
/// `wfi`, unmasking interrupts only for a moment after each wake, as the boot core's
/// idle loop does: its IPI line ends the wait, and the interrupt is a kernel entry of
/// its own. It parks when another core has panicked. Its `TTBR0_EL1` still names
/// Kickstart's identity map, which is Kickstart's memory, so translation walks through
/// `TTBR0` stop here; its timers are turned off, since their reset state is UNKNOWN and
/// a pending one would end every wait.
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.bootstrap")]
pub extern "C" fn nucleus_secondary_idle() -> ! {
    use aarch64_cpu::{
        asm::barrier,
        registers::{CNTP_CTL_EL0, CNTV_CTL_EL0, Read, TCR_EL1, Write},
    };
    TCR_EL1.modify(TCR_EL1::EPD0::DisableTTBR0Walks);
    barrier::isb(barrier::SY);
    // SAFETY: dropping this core's own TLB entries only costs walks.
    unsafe {
        asm!(
            "tlbi vmalle1",
            "dsb nsh",
            "isb",
            options(nostack, preserves_flags)
        );
    }
    CNTP_CTL_EL0.write(CNTP_CTL_EL0::ENABLE::CLEAR + CNTP_CTL_EL0::IMASK::SET);
    CNTV_CTL_EL0.write(CNTV_CTL_EL0::ENABLE::CLEAR + CNTV_CTL_EL0::IMASK::SET);
    let record = nucleus::core_record::current();
    let mut asked_controller = false;
    let mut takes_interrupts = false;
    loop {
        if another_core_panicked() {
            park_for_panic()
        }
        if !asked_controller && nucleus::platform::is_installed() {
            asked_controller = true;
            let _kernel = KERNEL_LOCK.enter();
            takes_interrupts = nucleus::platform::init_cpu(record.index);
        }
        if takes_interrupts {
            record.idling.store(true, Ordering::Release);
            aarch64_cpu::asm::wfi();
            libexception::asynchronous::local_irq_unmask();
            barrier::isb(barrier::SY);
            libexception::asynchronous::local_irq_mask();
            record.idling.store(false, Ordering::Release);
        } else {
            aarch64_cpu::asm::wfe();
        }
    }
}

/// Print `line` on the board console, when Kickstart gave one.
fn board_line(line: core::fmt::Arguments) {
    board_console::line(line);
}

/// Print a line on semihosting (under `qemu`) and on the board console.
macro_rules! board_println {
    ($($arg:tt)+) => {{
        semi::println!($($arg)+);
        board_line(format_args!($($arg)+));
    }};
}

/// Threads seen running, for [`first_run`]: their addresses. Only loaded and
/// stored, under the kernel lock or with interrupts masked on the one core.
static SEEN_THREADS: [AtomicU64; 64] = [const { AtomicU64::new(0) }; 64];

/// Whether this is the first time the Thread at `address` runs (while
/// [`SEEN_THREADS`] has room; after that, never).
fn first_run(address: u64) -> bool {
    for seen in &SEEN_THREADS {
        match seen.load(Ordering::Relaxed) {
            0 => {
                seen.store(address, Ordering::Relaxed);
                return true;
            }
            known if known == address => return false,
            _ => {}
        }
    }
    false
}

/// Whether the nucleus has gone idle before. Only loaded and stored.
static IDLED: AtomicBool = AtomicBool::new(false);

/// The boot-carved [`Nucleus`], or `None` before Kickstart records it.
pub fn nucleus_anchor() -> Option<*mut Nucleus<nucleus::objects::ArchObjectsImpl>> {
    let addr = NUCLEUS.load(Ordering::Relaxed);
    if addr == 0 {
        return None;
    }
    // SAFETY: Kickstart wrote `addr` as the address of the boot-carved Nucleus.
    Some(unsafe { addr as *mut Nucleus<nucleus::objects::ArchObjectsImpl> })
}

#[panic_handler]
fn panicked(info: &PanicInfo) -> ! {
    libexception::asynchronous::local_irq_mask();
    claim_panic();
    // Route panic output through the console logger: without an installed
    // logger, liblog drops every message on the default NopLogger, so a
    // nucleus panic would silently degrade into a bare hang. In QEMU builds
    // the logger mirrors to the semihosting console. The only install error
    // is "a logger is already installed", which is fine here.
    let _logger = libconsole::init_logger();
    // Take the console UART over from its driver, which never runs again, so
    // that the panic shows on the board too.
    if board_console::attached() {
        libconsole::console::register_console(&board_console::BOARD_UART);
    }
    if libstacktrace::ENABLED {
        println!(
            "nucleus panic {}",
            Backtrace::new(None, current_frame_pointer(), kernel_word)
        );
    }
    libmachine::panic::handler(info)
}

/// Where the kernel half starts: the `TTBR1` range of a 48-bit address space.
const KERNEL_HALF: u64 = 0xFFFF_0000_0000_0000;

/// The 64-bit word at `address` of the nucleus's own memory, for a stack
/// trace: only in the kernel half, where its stacks are, and only where an
/// EL1 read translates, so that a broken frame chain cannot fault the panic.
fn kernel_word(address: u64) -> Option<u64> {
    if address < KERNEL_HALF || !user_memory::readable(address, 8, user_memory::Reader::El1) {
        return None;
    }
    // SAFETY: the word is in the kernel half and translates for an EL1 read.
    unsafe { read_own(address) }
}

//------------------------------------------------------------------------------
//------------------------------------------------------------------------------
// Exception handlers
//------------------------------------------------------------------------------
//------------------------------------------------------------------------------

/// The default exception handler, invoked for every exception type unless the handler
/// is overridden.
/// Prints verbose information about the exception and then panics.
///
/// Default pointer is configured in the linker script.
#[unsafe(no_mangle)]
extern "C" fn default_exception_handler(exc: &ExceptionContext) {
    // In a build with stack traces, the trace from the exception's PC along
    // the interrupted code's frame chain (nucleus code only: `kernel_word`).
    panic!(
        "Unexpected CPU Exception!\n\n\
        {exc}\n{}",
        Backtrace::new(Some(exc.elr_el1), exc.gpr[29], kernel_word)
    );
}

//------------------------------------------------------------------------------
// Current, EL0
//------------------------------------------------------------------------------

#[unsafe(no_mangle)]
extern "C" fn current_el0_synchronous(e: &mut ExceptionContext) {
    // This vector is current EL using SP_EL0 (trusted EL1t), not lower EL0.
    // Exception entry has selected the shared SP_EL1 trap stack. It can
    // switch to an EL0 Thread in an FP AddressSpace, so it takes the guard.
    let _fp_guard = FpEntryGuard::enter();
    current_elx_synchronous(e);
}

#[unsafe(no_mangle)]
extern "C" fn current_el0_irq(e: &mut ExceptionContext) {
    // An interruptible trusted `EL1t` Thread (e.g. idle).
    let _fp_guard = FpEntryGuard::enter();
    let _kernel = KERNEL_LOCK.enter();
    handle_irq(e);
}

#[unsafe(no_mangle)]
extern "C" fn current_el0_serror(e: &mut ExceptionContext) {
    current_elx_serror(e);
}

//------------------------------------------------------------------------------
// Current, ELx
//------------------------------------------------------------------------------

#[cfg(not(any(test, feature = "test_build")))]
#[unsafe(no_mangle)]
extern "C" fn current_elx_synchronous(e: &mut ExceptionContext) {
    // Only an SVC in AArch64 state is a capability invocation. Any other
    // synchronous exception from the current EL (data/instruction abort,
    // alignment fault, undefined instruction) must not be decoded as a
    // syscall: its context would re-execute the faulting instruction with
    // the "syscall result" written into x0..x2, looping forever. Route it
    // to the default handler, which reports the exception and halts.
    #[cfg(feature = "fp_trap_test")]
    if absorb_fp_simd_trap(e) {
        return;
    }
    if !is_aarch64_svc() {
        default_exception_handler(e);
    }
    let _kernel = KERNEL_LOCK.enter();
    #[cfg(feature = "smp_test")]
    if smp_test_ping(e) {
        return;
    }
    cap_invoke_handler(e);
}

/// `smp-test`'s ping SVC (`core_record::SMP_TEST_PING_SVC`): send `IPI_PING` to the
/// logical CPU in `x0`, which answers this core with `IPI_PONG`; `x0` returns 0 once
/// the controller sent it, 1 otherwise. Whether the SVC was the ping. Runs under the
/// kernel lock; the pinged core takes its IPI once this entry let the lock go.
#[cfg(feature = "smp_test")]
fn smp_test_ping(e: &mut ExceptionContext) -> bool {
    use aarch64_cpu::registers::{ESR_EL1, Read};
    if ESR_EL1.read(ESR_EL1::ISS) != u64::from(nucleus::core_record::SMP_TEST_PING_SVC) {
        return false;
    }
    let cpu = e.gpr[0];
    // The kinds' Release in `send_ipis` publishes this to the pinged core.
    PINGING_CORE.store(nucleus::core_record::current().index, Ordering::Relaxed);
    let sent = cpu < u64::from(u32::BITS) && send_ipis(1 << cpu, nucleus::core_record::IPI_PING);
    e.gpr[0] = u64::from(!sent);
    true
}

#[cfg(any(test, feature = "test_build"))]
#[unsafe(no_mangle)]
extern "C" fn current_elx_synchronous(e: &mut ExceptionContext) {
    {
        use aarch64_cpu::registers::{ESR_EL1, Read};

        const TEST_SVC_ID: u64 = 0x1337;

        let esr_el1 = libexception::arch::esr_el1::EsrEL1(LocalRegisterCopy::new(ESR_EL1.get()));

        if let Some(ESR_EL1::EC::Value::SVC64) = esr_el1.exception_class()
            && esr_el1.iss() == TEST_SVC_ID
        {
            liblog::println!("Serving syscall {TEST_SVC_ID}");
            return;
        }
    }

    if libdebug::exception_dump(e) {
        return;
    }

    default_exception_handler(e);
}

#[unsafe(no_mangle)]
extern "C" fn current_elx_irq(e: &mut ExceptionContext) {
    // The nucleus runs with interrupts masked, except where it waits for one
    // while idle: an IRQ taken on SP_EL1 anywhere else means kernel code
    // unmasked them, which is a bug.
    if nucleus::core_record::current()
        .idling
        .load(Ordering::Acquire)
    {
        // The idle loop let the lock go around its wait: this interrupt is an entry
        // of its own.
        let _kernel = KERNEL_LOCK.enter();
        handle_irq(e);
    } else {
        default_exception_handler(e);
    }
}

#[unsafe(no_mangle)]
extern "C" fn current_elx_serror(e: &mut ExceptionContext) {
    default_exception_handler(e);
}

//------------------------------------------------------------------------------
// Lower, AArch64
//------------------------------------------------------------------------------

#[unsafe(no_mangle)]
extern "C" fn lower_aarch64_synchronous(e: &mut ExceptionContext) {
    let _fp_guard = FpEntryGuard::enter();
    #[cfg(feature = "fp_trap_test")]
    if run_kernel_fp_probe(e) {
        return;
    }
    let _kernel = KERNEL_LOCK.enter();
    // Only an SVC is a capability invocation; every other synchronous
    // exception from EL0 is a fault, delivered to the fault handler.
    if !is_aarch64_svc() {
        use aarch64_cpu::registers::{ESR_EL1, FAR_EL1, Read};

        let faulting = e.save();
        let info = FaultInfo {
            kind: FaultKind::CpuException,
            esr: ESR_EL1.get(),
            far: FAR_EL1.get(),
            pc: faulting.elr_el1,
            sp: faulting.sp,
            depth: current_invocation_depth(),
        };
        deliver_fault(e, &faulting, info);
        reprogram_timer();
        return;
    }
    cap_invoke_handler(e);
}

/// The current Thread's invocation depth, for fault reports (0 if unknown).
fn current_invocation_depth() -> u64 {
    KERNEL_LOCK
        .held(|()| {
            let nucleus = nucleus_anchor()?;
            // SAFETY: the anchor names the live Nucleus; the lock serializes
            // this read.
            let nucleus = unsafe { &*nucleus };
            let thread = nucleus.current_thread_ref().ok()?;
            u64::try_from(thread.invocation_stack.len()).ok()
        })
        .unwrap_or(0)
}

#[unsafe(no_mangle)]
extern "C" fn lower_aarch64_irq(e: &mut ExceptionContext) {
    let _fp_guard = FpEntryGuard::enter();
    let _kernel = KERNEL_LOCK.enter();
    handle_irq(e);
}

#[unsafe(no_mangle)]
extern "C" fn lower_aarch64_serror(e: &mut ExceptionContext) {
    default_exception_handler(e);
}

//------------------------------------------------------------------------------
// Lower, AArch32
//------------------------------------------------------------------------------

#[unsafe(no_mangle)]
extern "C" fn lower_aarch32_synchronous(e: &mut ExceptionContext) {
    default_exception_handler(e);
}

#[unsafe(no_mangle)]
extern "C" fn lower_aarch32_irq(e: &mut ExceptionContext) {
    default_exception_handler(e);
}

#[unsafe(no_mangle)]
extern "C" fn lower_aarch32_serror(e: &mut ExceptionContext) {
    default_exception_handler(e);
}

/// Test-only hook for trusted `EL1t` code (`fp-trap-test`): faults in `EL1t` code
/// halt the kernel (fault delivery covers EL0 only), so to check that FP/SIMD
/// traps at `EL1t` this absorbs an FP/SIMD access trap (`ESR_EL1.EC` 0x07) by
/// handing its syndrome back to the faulting code in `x0` and resuming after
/// the trapping instruction. Production builds halt on it like any `EL1t` fault.
#[cfg(feature = "fp_trap_test")]
fn absorb_fp_simd_trap(e: &mut ExceptionContext) -> bool {
    use aarch64_cpu::registers::{ESR_EL1, Read};

    if !ESR_EL1.matches_all(ESR_EL1::EC::TrappedFP) {
        return false;
    }
    semi::println!(
        "🧪 FP/SIMD access trapped from {:?} at {:#x}, ESR {:#x}",
        e.origin,
        e.elr_el1,
        ESR_EL1.get()
    );
    e.gpr[0] = ESR_EL1.get();
    // Every A64 instruction is four bytes; the trap is taken before it executes.
    e.elr_el1 += 4;
    true
}

/// Test-only hook for `fp-trap-test`'s kernel probe: a `brk` with
/// `fp_trap_protocol::KERNEL_PROBE_BRK` from EL0 makes the nucleus execute the
/// probe's FP/SIMD instruction itself, while serving that frame, and hand its
/// `x0` back past the `brk`. A trap at EL1 comes back through
/// `absorb_fp_simd_trap` as the syndrome.
#[cfg(feature = "fp_trap_test")]
fn run_kernel_fp_probe(e: &mut ExceptionContext) -> bool {
    use aarch64_cpu::registers::{ESR_EL1, Read};

    if !fp_trap_protocol::is_kernel_probe(ESR_EL1.get()) {
        return false;
    }
    e.gpr[0] = fp_trap_protocol::probe();
    e.elr_el1 += 4;
    true
}

/// Whether the synchronous exception being handled is an SVC instruction
/// executed in `AArch64` state — the only exception class that carries a
/// capability invocation. The SVC immediate is deliberately ignored: there is
/// one syscall, so every `svc #n` is the same invocation (see the contract's
/// ordinary control invocation baseline).
fn is_aarch64_svc() -> bool {
    use aarch64_cpu::registers::{ESR_EL1, Read};

    // ESR_EL1 bits 31:26 hold the exception class; 0x15 is an SVC
    // instruction executed in AArch64 state (see
    // `libexception::arch::esr_el1::ESR_EL1::EC::Value::SVC64`).
    const EC_SVC64: u64 = 0x15;
    (ESR_EL1.get() >> 26) & 0x3F == EC_SVC64
}

//------------------------------------------------------------------------------
// Kernel entry point
//------------------------------------------------------------------------------

use owo_colors::OwoColorize;

#[unsafe(no_mangle)]
extern "C" fn cap_invoke_handler(frame: &mut ExceptionContext) {
    cap_invoke_dispatch(frame);
    reprogram_timer();
}

/// Decode and run one capability invocation from the saved frame, then
/// rewrite the frame for whatever Thread continues.
fn cap_invoke_dispatch(frame: &mut ExceptionContext) {
    let key = RawKey::from_wire(frame.gpr[0]);
    let op = frame.gpr[1];
    // semi::println!(
    //     "➡️  {}(key: {key:?}, op: {op}) @ PC {:#016X}, SP {:#016X}, exception frame @ {:#016X}",
    //     "CapInvoke SYSCALL".dimmed(),
    //     get_pc(),
    //     get_sp(),
    //     core::ptr::from_mut(frame) as u64,
    // );

    // SP_EL1 belongs to the active kernel, not a schedulable caller. Gate
    // provenance before dispatch can reserve a pending record or mutate queues.
    if !matches!(
        frame.origin,
        ExceptionOrigin::CurrentSp0 | ExceptionOrigin::LowerAarch64
    ) {
        let (status, detail0, detail1) = CapError::InvalidThread.code();
        frame.gpr[0] = status;
        frame.gpr[1] = detail0;
        frame.gpr[2] = detail1;
        return;
    }

    // Dispatch reads every input from this copy of the saved frame,
    // including PPC Call's x9 target SP; nothing after Rust entry uses live
    // argument registers.
    let saved = frame.save();

    // SAFETY: Unsafe.
    let outcome = unsafe {
        #[allow(static_mut_refs)]
        KERNEL_LOCK.held(|()| {
            let Some(ptr) = nucleus_anchor() else {
                panic!("nucleus not booted by Kickstart")
            };
            // SAFETY: the anchor points at the live boot-carved Nucleus.
            nucleus::api::handle_cap_invoke(unsafe { &mut *ptr }, &saved)
        })
    };

    // A blocked invocation cannot return to its caller yet. Copy its state
    // into the Thread, then rewrite this transient frame for the selected
    // Thread. The handler unwinds normally; vectors restore it with ERET.
    // Dispatch guards have ended before the scheduling transaction begins.
    let (x0, x1, x2) = match outcome {
        Ok(nucleus::api::InvokeOutcome::Complete((v0, v1))) => (syscall_status::SUCCESS, v0, v1),
        Ok(nucleus::api::InvokeOutcome::Activate(prepared)) => {
            // API/Access guards and KERNEL_LOCK have ended. Execution and
            // SP_EL1 use the invariant high TTBR1 map, not the replaced root.
            // Trap entry masks interrupts; no scheduling/reentry occurs before
            // this immediate installation on the enforced single boot core.
            ArchObjectsImpl::install_translation_context(
                prepared.root(),
                prepared.asid(),
                prepared.fp_simd(),
            );
            semi::println!("{}", "✅ AddressSpace::Activate()".on_cyan());
            (syscall_status::SUCCESS, 0, 0)
        }
        Ok(nucleus::api::InvokeOutcome::Blocked(record)) => {
            park_and_resume(frame, record);
            return;
        }
        Ok(nucleus::api::InvokeOutcome::Exit) => {
            // The Thread is gone if this succeeds; if nothing else can run,
            // the caller sees the error and carries on.
            match exit_current(frame) {
                None => return,
                Some(code) => code,
            }
        }
        Ok(nucleus::api::InvokeOutcome::Call(committed)) => {
            // Committed under the lock: continuation pushed, Thread migrated.
            // Guards and KERNEL_LOCK have ended; install the target context,
            // then rewrite this transient frame with the scrubbed target
            // entry. Vectors ERET into the target on the same Thread.
            install(committed.translation);
            frame.restore(committed.target);
            if committed.with_key {
                semi::println!("{}", "✅ Invocation::CallWithKey()".on_cyan());
            } else {
                semi::println!("{}", "✅ Invocation::Call()".on_cyan());
            }
            return;
        }
        Ok(nucleus::api::InvokeOutcome::Return(committed)) if committed.terminate => {
            // A fault handler chose to terminate its faulting Thread.
            semi::println!("{}", "✅ Thread::Return()".on_cyan());
            terminate_faulted(frame, &committed.resumed);
            return;
        }
        Ok(nucleus::api::InvokeOutcome::Fault(kind)) => {
            // A Return protocol fault, delivered like any EL0 fault. Faults in
            // trusted EL1t code halt the kernel.
            assert!(
                frame.origin == ExceptionOrigin::LowerAarch64,
                "Thread.Return fault in trusted EL1t code: {kind:?}"
            );
            // The fault is at the Return's `svc`: ELR already points past it.
            let mut faulting = saved;
            faulting.elr_el1 = faulting.elr_el1.wrapping_sub(4);
            let info = FaultInfo {
                kind,
                esr: 0,
                far: 0,
                pc: faulting.elr_el1,
                sp: faulting.sp,
                depth: current_invocation_depth(),
            };
            deliver_fault(frame, &faulting, info);
            return;
        }
        Ok(nucleus::api::InvokeOutcome::Return(committed)) => {
            // Committed under the lock: top continuation popped, Thread back
            // in its source AddressSpace. Install, then resume the source with
            // SUCCESS/r0/r1 and its exact saved callee-saved state (or, ending
            // a fault handler, the faulting state as its action selects).
            install(committed.translation);
            frame.restore(committed.resumed);
            semi::println!("{}", "✅ Thread::Return()".on_cyan());
            return;
        }
        Err(e) => e.code(),
    };
    // Return values
    // semi::println!(
    //     "⬅️  {}(Return {x0:#x}, {x1:#x}, {x2:#x})",
    //     "CapInvoke SYSCALL".dimmed()
    // );
    frame.gpr[0] = x0;
    frame.gpr[1] = x1;
    frame.gpr[2] = x2;
}

// ═══════════════════════════════════════════════════════════════════
// INTERRUPTS
// ═══════════════════════════════════════════════════════════════════

/// Upper bound on lines claimed per interrupt entry, so a misbehaving
/// controller cannot keep the nucleus in the handler.
const MAX_CLAIMS_PER_ENTRY: usize = 64;

/// Interrupt entry from an interruptible Thread: claim and handle every
/// pending line through the interrupt-controller component, then, on a
/// timer tick (or when a dispatcher was activated), preempt the current
/// Thread.
///
/// The timer line re-arms the tick. Every other line is masked and ended;
/// a line bound by `IRQHandler.SetNotification` also signals its
/// Notification, and stays masked until the handler's `Ack`.
///
/// An interrupt that ends the kernel's idle wait (taken on `SP_EL1`) only
/// claims, delivers, accounts and expires deadlines; the idle loop selects.
fn handle_irq(frame: &mut ExceptionContext) {
    let nested = frame.origin == ExceptionOrigin::CurrentSpx;
    let timer_line = nucleus::platform::physical_timer_line();
    let ipi_line = nucleus::platform::ipi_line();
    let mut ticked = false;
    let mut reschedule = false;
    // Interrupt handling is timed, line by line and in all: the interrupted
    // Thread is not charged for it, and each line's record counts it.
    let entered = nucleus::timer::counter();
    let mut handled = [(0_u32, 0_u64); MAX_CLAIMS_PER_ENTRY];
    let mut handled_count = 0;
    for _ in 0..MAX_CLAIMS_PER_ENTRY {
        let claim_started = nucleus::timer::counter();
        let Some(line) = nucleus::platform::claim() else {
            break;
        };
        if Some(line) == timer_line {
            // One-shot: deassert it here; the kernel exit programs the next
            // deadline, if any.
            nucleus::timer::disarm();
            ticked = true;
        } else if Some(line) == ipi_line {
            // The claim acknowledged it; the kinds are in this core's record.
            reschedule |= take_ipis();
        } else {
            nucleus::platform::mask(line);
            if !deliver_irq(line) {
                warn!("Masked unbound interrupt line {line}");
            }
        }
        nucleus::platform::complete(line);
        let spent = nucleus::timer::counter().saturating_sub(claim_started);
        handled[handled_count] = (line, spent);
        handled_count += 1;
    }
    let interrupt_ticks = nucleus::timer::counter().saturating_sub(entered);

    let saved = frame.save();
    let resumed = KERNEL_LOCK.held(|()| {
        let Some(nucleus_ptr) = nucleus_anchor() else {
            panic!("nucleus not booted by Kickstart")
        };
        // SAFETY: the anchor names the retained boot-carved Nucleus, and the
        // kernel lock gives exclusive access for this scheduling transaction.
        let nucleus = unsafe { &mut *nucleus_ptr };
        nucleus.account_interrupts(&handled[..handled_count], interrupt_ticks);
        if ticked {
            nucleus.ticks += 1;
            // Whatever is due wakes: timed-out waits, dispatcher timers.
            nucleus.expire_deadlines();
        }
        if reschedule {
            // Another core changed what may run here: select on the way out,
            // as for an activation.
            nucleus.this_core.preempt_requested = true;
        }
        if nested || (!ticked && !nucleus.this_core.preempt_requested) {
            return Ok(None);
        }
        // SAFETY: exclusive access is serialized by KERNEL_LOCK and no other
        // Access or guard overlaps this transaction.
        let access = unsafe { Access::new() };
        nucleus.preempt_and_select(&access, saved)
    });
    match resumed {
        Ok(Some(Switch::Resume(resumed))) => install_selected(frame, &resumed),
        Ok(Some(Switch::Idle)) => {
            let resumed = idle_until_runnable();
            install_selected(frame, &resumed);
        }
        Ok(None) => {}
        Err(error) => panic!("preemption scheduling invariant failed: {:?}", error.code()),
    }
    reprogram_timer();
}

/// Wait in the kernel until a Thread is runnable, then select it.
///
/// Nothing is current. With interrupts masked the nucleus looks for a
/// runnable Thread; finding none it waits: `wfi` wakes for a pending
/// interrupt even while masked, and unmasking takes it on `SP_EL1` (the
/// restricted path of [`handle_irq`]). The trap frame that led here stays
/// untouched below, for the caller to rewrite with the selected Thread.
fn idle_until_runnable() -> PreparedResume {
    if !IDLED.load(Ordering::Relaxed) {
        IDLED.store(true, Ordering::Relaxed);
        board_line(format_args!("💤 nothing runnable: idle, the first time"));
    }
    loop {
        // The timer runs only for a wait timeout or a dispatcher's timer.
        reprogram_timer();
        let selected = KERNEL_LOCK.held(|()| {
            let Some(nucleus_ptr) = nucleus_anchor() else {
                panic!("nucleus not booted by Kickstart")
            };
            // SAFETY: the anchor names the retained boot-carved Nucleus, and
            // the kernel lock gives exclusive access for this transaction.
            let nucleus = unsafe { &mut *nucleus_ptr };
            // SAFETY: exclusive access is serialized by KERNEL_LOCK and no
            // other Access or guard overlaps this transaction.
            let access = unsafe { Access::new() };
            nucleus.select_after_idle(&access)
        });
        match selected {
            Ok(Some(resumed)) => return resumed,
            Ok(None) => {}
            Err(error) => panic!("idle scheduling invariant failed: {:?}", error.code()),
        }
        // Whether this core waits for an interrupt with nothing to run, the
        // one place kernel code is interruptible.
        let idling = &nucleus::core_record::current().idling;
        idling.store(true, Ordering::Release);
        // Other cores may enter the kernel while this one waits; the interrupt
        // taken in the window below takes the lock as an entry of its own.
        KERNEL_LOCK.release_around(|| {
            aarch64_cpu::asm::wfi();
            libexception::asynchronous::local_irq_unmask();
            aarch64_cpu::asm::barrier::isb(aarch64_cpu::asm::barrier::SY);
            libexception::asynchronous::local_irq_mask();
        });
        idling.store(false, Ordering::Release);
    }
}

/// Program this core's timer for the next instant the nucleus needs it here,
/// from the state the kernel entry just left: the end of the current slice
/// when another Thread is runnable, the dispatcher's limit, the exhaustion of
/// the current Thread's budget, or a wait timeout or dispatcher timer this
/// core counts. Every kernel exit that can change one of them calls this.
fn reprogram_timer() {
    let deadline = KERNEL_LOCK.held(|()| {
        let nucleus_ptr = nucleus_anchor()?;
        // SAFETY: the anchor names the retained boot-carved Nucleus, and the
        // kernel lock gives exclusive access.
        let nucleus = unsafe { &mut *nucleus_ptr };
        nucleus.timer_deadline_ticks(nucleus::timer::slice_ticks())
    });
    nucleus::timer::program(deadline);
}

/// Signal the Notification bound to the claimed `line`, if any; whether
/// one was signalled.
fn deliver_irq(line: u32) -> bool {
    KERNEL_LOCK.held(|()| {
        let Some(nucleus_ptr) = nucleus_anchor() else {
            panic!("nucleus not booted by Kickstart")
        };
        // SAFETY: the anchor names the retained boot-carved Nucleus, and the
        // kernel lock gives exclusive access for this delivery.
        let nucleus = unsafe { &mut *nucleus_ptr };
        // SAFETY: exclusive access is serialized by KERNEL_LOCK and no other
        // Access or guard overlaps this transaction.
        let access = unsafe { Access::new() };
        nucleus::api::irq::deliver(&access, line, nucleus)
    })
}

// ═══════════════════════════════════════════════════════════════════
// CONTEXT SWITCHING
// ═══════════════════════════════════════════════════════════════════

/// Park the current Thread in private storage and select a runnable context.
///
/// Dispatch has released its guards before this transaction begins. No frame
/// address survives it: the current per-core frame is rewritten after the
/// lock ends, and the Rust handler returns normally to the vector epilogue.
/// Historical prerequisite status, superseded by the implementation below:
/// hardware `AddressSpace` installation was pending, with the trusted Bounce
/// fixture running under the bootstrap translation root.
/// Implementation status: checked preparation/commit now lives in
/// `Nucleus::park_and_select`; installation follows the lock below. Bootstrap
/// must provision complete source/target roots before admitting the first wait.
fn park_and_resume(frame: &mut ExceptionContext, record: ObjectId) {
    assert!(
        frame.origin != ExceptionOrigin::CurrentSpx,
        "kernel-mode execution cannot retain a continuation on SP_EL1"
    );
    let saved = frame.save();
    let resumed = KERNEL_LOCK.held(|()| {
        let Some(nucleus_ptr) = nucleus_anchor() else {
            panic!("nucleus not booted by Kickstart")
        };
        // SAFETY: the anchor names the retained boot-carved Nucleus, and the
        // kernel lock gives exclusive access for this scheduling transaction.
        let nucleus = unsafe { &mut *nucleus_ptr };
        // SAFETY: exclusive access is serialized by KERNEL_LOCK, dispatch's
        // Access has ended, and no second context overlaps this transaction.
        let access = unsafe { Access::new() };
        nucleus.park_and_select(&access, saved, record)
    });

    // With nothing runnable the kernel idles (see `switch_to`). Scheduler-error
    // recovery/fault delivery has no ABI yet; trusted fixture corruption is an
    // invariant failure, after a failure-atomic preparation (not a lost wait).
    let switch = resumed.unwrap_or_else(|error| {
        panic!(
            "wait/resume scheduling invariant failed before commit: {:?}",
            error.code()
        )
    });
    switch_to(frame, &switch, "parked");
}

/// Install the Thread a scheduling transaction selected and rewrite the
/// transient frame with its context, without tracing (the preemption path).
fn install_selected(
    frame: &mut ExceptionContext,
    resumed: &nucleus::objects::resume::PreparedResume,
) {
    // Only copied prepared metadata and the transient frame are live here;
    // no Thread or object guard crosses hardware installation or ERET.
    // High kernel code and shared high SP_EL1 remain mapped by TTBR1. The
    // single-core, masked trap path cannot retire/rebind between prepare and
    // install; this is not a reusable lifetime pin for asynchronous work.
    install(resumed.translation);
    // SP_EL1 is reclaimed by normal unwinding, with no kernel stack switch.
    frame.restore(resumed.saved);
}

/// Install a prepared translation context: its root and ASID, and the FP
/// trap its `AddressSpace` needs.
fn install(translation: PreparedTranslationContext) {
    ArchObjectsImpl::install_translation_context(
        translation.root(),
        translation.asid(),
        translation.fp_simd(),
    );
}

/// Enter what a scheduling transaction decided: the selected Thread, or,
/// with nothing runnable, the first Thread to become runnable.
fn switch_to(frame: &mut ExceptionContext, switch: &Switch, why: &str) {
    match switch {
        Switch::Resume(resumed) => resume_selected(frame, resumed, why),
        Switch::Idle => {
            semi::println!("💤 nothing runnable ({why}): idle");
            let resumed = idle_until_runnable();
            resume_selected(frame, &resumed, "idle");
        }
    }
}

/// Install and enter the Thread a scheduling transaction selected: rewrite
/// the transient frame with its context and trace the switch. `why` names
/// what happened to the Thread that stopped.
fn resume_selected(
    frame: &mut ExceptionContext,
    resumed: &nucleus::objects::resume::PreparedResume,
    why: &str,
) {
    let next = resumed.next;
    let restored = resumed.saved;
    install_selected(frame, resumed);
    if let Some(completion) = resumed.completion {
        let kind = match completion.kind {
            PendingKind::NotificationAwait => "Notification::Await",
            PendingKind::EventCountAwait => "EventCount::Await",
            PendingKind::WaitSetAwait => "WaitSet::Await",
            PendingKind::SchedulerDispatch => "Scheduler::Dispatch",
        };
        let status = completion.status;
        let result0 = completion.result0;
        if status == syscall_status::SUCCESS {
            semi::println!("✅ {kind}(0x{result0:x}) resumed");
        } else {
            semi::println!("⬅️ {kind} resumed with status {status:#x}");
        }
    }
    semi::println!(
        "🔄 context switch: thread {:#x} {why}, resuming thread {:#x} @ SP {:#x}, PC {:#x}",
        resumed.current.map_or(0, |current| current.address),
        next.address,
        restored.sp,
        restored.elr_el1,
    );
    if first_run(next.address) {
        board_line(format_args!(
            "🧵 thread {:#x} runs for the first time, PC {:#x}, SP {:#x} (thread {:#x} {why})",
            next.address,
            restored.elr_el1,
            restored.sp,
            resumed.current.map_or(0, |current| current.address),
        ));
    }
}

// ═══════════════════════════════════════════════════════════════════
// FAULT DELIVERY
// ═══════════════════════════════════════════════════════════════════

/// Deliver a fault on the current Thread: a synchronous upcall into its
/// `AddressSpace`'s fault handler, or — if nobody takes it — park the Thread
/// as faulted and resume the next one. `faulting` is the complete faulting
/// state (for a Return fault, with PC at its `svc`).
fn deliver_fault(frame: &mut ExceptionContext, faulting: &SavedContext, info: FaultInfo) {
    let thread = KERNEL_LOCK
        .held(|()| {
            // SAFETY: the anchor names the live Nucleus; the lock serializes
            // this read.
            let nucleus = unsafe { &*nucleus_anchor()? };
            nucleus.current_thread_id().ok()
        })
        .map_or(0, |current| current.address);
    board_println!(
        "⚠️  fault {:?} in thread {thread:#x} at PC {:#x}, ESR {:#x}, FAR {:#x}, depth {}",
        info.kind,
        info.pc,
        info.esr,
        info.far,
        info.depth
    );
    if libstacktrace::ENABLED {
        // The faulting Thread's AddressSpace is still the active one.
        board_println!(
            "   {}",
            Backtrace::new(Some(info.pc), faulting.gpr[29], user_memory::read_el0_word)
        );
    }
    let delivery = KERNEL_LOCK.held(|()| {
        let Some(nucleus_ptr) = nucleus_anchor() else {
            panic!("nucleus not booted by Kickstart")
        };
        // SAFETY: the anchor names the retained boot-carved Nucleus; the
        // kernel lock gives exclusive access for this transaction.
        let nucleus = unsafe { &mut *nucleus_ptr };
        // SAFETY: exclusive access is serialized by KERNEL_LOCK and no other
        // Access or guard overlaps this transaction.
        let access = unsafe { Access::new() };
        nucleus.deliver_fault(&access, *faulting, info)
    });
    match delivery {
        Ok(FaultDelivery::Handler(committed)) => {
            install(committed.translation);
            frame.restore(committed.target);
            semi::println!("{}", "✅ fault delivered to its handler".on_cyan());
        }
        Ok(FaultDelivery::Unhandled(resumed)) => {
            board_println!("⛔ unhandled fault: thread parked as faulted");
            switch_to(frame, &resumed, "faulted");
        }
        // Nothing is left to run (or scheduling state is corrupt).
        Err(error) => panic!("fault left nothing to run: {:?}", error.code()),
    }
}

/// `Thread.Retire` on the calling Thread: switch to the next Thread and
/// tear the caller down. `None` once it has switched; the error words when
/// nothing could run and the caller continues.
fn exit_current(frame: &mut ExceptionContext) -> Option<(u64, u64, u64)> {
    let resumed = KERNEL_LOCK.held(|()| {
        let Some(nucleus_ptr) = nucleus_anchor() else {
            panic!("nucleus not booted by Kickstart")
        };
        // SAFETY: the anchor names the retained boot-carved Nucleus, and the
        // kernel lock gives exclusive access for this scheduling transaction.
        let nucleus = unsafe { &mut *nucleus_ptr };
        // SAFETY: exclusive access is serialized by KERNEL_LOCK and no other
        // Access or guard overlaps this transaction.
        let access = unsafe { Access::new() };
        nucleus.exit_current_thread(&access)
    });
    match resumed {
        Ok(resumed) => {
            board_println!("✅ Thread::Retire() (self)");
            switch_to(frame, &resumed, "exited");
            None
        }
        Err(error) => Some(error.code()),
    }
}

/// A fault handler returned with the terminate action: park the Thread as
/// faulted with its faulting state and resume the next one.
fn terminate_faulted(frame: &mut ExceptionContext, faulting: &SavedContext) {
    let resumed = KERNEL_LOCK.held(|()| {
        let Some(nucleus_ptr) = nucleus_anchor() else {
            panic!("nucleus not booted by Kickstart")
        };
        // SAFETY: as in `deliver_fault`.
        let nucleus = unsafe { &mut *nucleus_ptr };
        // SAFETY: as in `deliver_fault`.
        let access = unsafe { Access::new() };
        nucleus
            .current_thread_id()
            .and_then(|current| nucleus.park_faulted(&access, current, *faulting))
    });
    match resumed {
        Ok(resumed) => {
            board_println!("⛔ fault handler terminated the thread");
            switch_to(frame, &resumed, "terminated");
        }
        Err(error) => panic!("fault left nothing to run: {:?}", error.code()),
    }
}

fn get_pc() -> u64 {
    let pc: u64;
    // SAFETY: Safe.
    unsafe {
        asm!(
            "adr {}, .",
            out(reg) pc,
        );
    }
    pc
}

fn get_sp() -> u64 {
    use aarch64_cpu::registers::Read;
    aarch64_cpu::registers::SP.get()
}
