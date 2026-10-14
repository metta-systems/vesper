//! Kernel-owned numeric stack contract carried inline in an Invocation.
//!
//! A validated extent establishes neither mapped/writable backing nor exclusive
//! stack ownership. Component setup supplies those guarantees; no allocation or
//! translation walk occurs here.
//!
//! `Nucleus::prepare_call` is the non-committing `Invocation.Call` admission
//! and preparation primitive; `Nucleus::commit_call` is its stage-5 commit.
//! `Nucleus::prepare_return`/`commit_return` are the matching `Thread.Return`
//! primitives. `api::handle_cap_invoke` dispatches both.

use {
    crate::{
        api::key_entry::KeyEntry,
        objects::{
            ArchObjects, ExecutionContext, InvocationContinuation, KeyTable, Nucleus, Placement,
            access::Access,
            arch_objects::{AddressSpaceId, AddressSpaceObject},
            carved::CarvedId,
            key_table::CallerTable,
            resume::{PreparedTranslationContext, execution_origin, prepare_translation_context},
            thread::ThreadId,
        },
    },
    core::num::NonZero,
    libexception::arch::aarch64::SavedContext,
    libobject::{
        CapError, InvalidStackReason, KeySlot, PlacementWindow, RawKey, Rights, fault::FaultAction,
        syscall_status::InvalidSelfReference,
    },
};

/// The input register that carries the lent key of `CallWithKey` (`x8`), as
/// key diagnostics report it.
const LENT_KEY_OPERAND: u8 = 8;

/// An immutable, validated target stack extent and downward headroom requirement.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvocationStackExtent {
    base: u64,
    end: u64,
    minimum_headroom: u64,
}

impl InvocationStackExtent {
    /// Validate the published extent within `[0, user_end_exclusive)`.
    /// The exclusive extent end may equal the user ceiling. Predicate order is
    /// part of the diagnostic ABI; compare bounds before subtracting.
    pub fn new(
        base: u64,
        end: u64,
        minimum_headroom: u64,
        user_end_exclusive: u64,
    ) -> Result<Self, CapError> {
        use InvalidStackReason as Reason;

        let invalid = |value, reason| CapError::InvalidStack { value, reason };
        if end == base {
            return Err(invalid(end, Reason::ExtentEmpty));
        }
        if end < base {
            return Err(invalid(end, Reason::ExtentInverted));
        }
        if base >= user_end_exclusive {
            return Err(invalid(base, Reason::BaseOutsideUserRange));
        }
        if end > user_end_exclusive {
            return Err(invalid(end, Reason::EndOutsideUserRange));
        }
        if base & 15 != 0 {
            return Err(invalid(base, Reason::BaseMisaligned));
        }
        if end & 15 != 0 {
            return Err(invalid(end, Reason::EndMisaligned));
        }
        if minimum_headroom == 0 {
            return Err(invalid(minimum_headroom, Reason::MinimumHeadroomZero));
        }
        if minimum_headroom & 15 != 0 {
            return Err(invalid(minimum_headroom, Reason::MinimumHeadroomMisaligned));
        }
        if minimum_headroom > end - base {
            return Err(invalid(minimum_headroom, Reason::MinimumHeadroomTooLarge));
        }
        Ok(Self {
            base,
            end,
            minimum_headroom,
        })
    }

    pub const fn base(self) -> u64 {
        self.base
    }

    pub const fn end(self) -> u64 {
        self.end
    }

    pub const fn minimum_headroom(self) -> u64 {
        self.minimum_headroom
    }

    /// Validate the submitted descending-stack SP before any Call admission.
    /// This helper does not enable Call or inspect mappings, depth or AS readiness.
    pub fn validate_sp(self, sp: u64) -> Result<(), CapError> {
        use InvalidStackReason as Reason;

        let invalid = |reason| CapError::InvalidStack { value: sp, reason };
        if sp & 15 != 0 {
            return Err(invalid(Reason::SpMisaligned));
        }
        if sp <= self.base || sp > self.end {
            return Err(invalid(Reason::SpOutOfRange));
        }
        if sp - self.base < self.minimum_headroom {
            return Err(invalid(Reason::SpInsufficientHeadroom));
        }
        Ok(())
    }
}

const _: () = assert!(core::mem::size_of::<InvocationStackExtent>() == 24);
const _: () = assert!(core::mem::align_of::<InvocationStackExtent>() == 8);

/// Checked Call target copied out of a looked-up `Invocation` entry.
///
/// The API layer constructs this after key lookup, Call-only operation and
/// `CALL` authority checks; it carries no entry reference or table guard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CallTarget {
    pub address_space: AddressSpaceId,
    pub function_address: NonZero<u64>,
    pub stack_extent: InvocationStackExtent,
    /// Where `CallWithKey` places a lent key in the target's table; empty if
    /// the Invocation accepts none.
    pub placement_window: PlacementWindow,
}

/// A key lent through `CallWithKey`, as the caller named it: its packed key
/// and the caller's own table context it resolves through.
///
/// No entry reference or table guard survives: the entry is looked up again
/// whenever it is needed within the serialized interval.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LendRequest {
    pub key: RawKey,
    pub caller: CallerTable,
}

/// The admitted placement of a lent key: where the borrowed entry will be
/// installed in the target's table, and the exact key the callee will receive
/// (the incarnation is known before installation).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PreparedPlacement {
    lent: LendRequest,
    target_table: CallerTable,
    slot: KeySlot,
    incarnation: u32,
    placed_key: RawKey,
}

/// A fully admitted, uncommitted `Invocation.Call` migration description.
///
/// Producing this value mutates nothing: the source Thread, its invocation
/// stack, the target translation context and every capability table are as
/// they were. It is not a lifetime pin or authority token. Like
/// [`PreparedTranslationContext`], it is valid only within the serialized
/// single-core trap interval that produced it; a later commit must not
/// interleave another transaction between preparation and push/switch.
///
/// Implementation status: `Nucleus::commit_call` consumes this description.
/// Translation install, frame rewrite and the success trace are entry duties;
/// `core_invoke` dispatches Call through this path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreparedCall {
    source_thread: ThreadId,
    continuation: InvocationContinuation,
    translation: PreparedTranslationContext,
    entry_pc: NonZero<u64>,
    target_sp: u64,
    arguments: [u64; 6],
    placement: Option<PreparedPlacement>,
    depth: usize,
}

impl PreparedCall {
    /// Checked identity of the migrating (current) Thread.
    pub const fn source_thread(&self) -> ThreadId {
        self.source_thread
    }

    /// Source continuation to push at commit, captured from the saved frame.
    pub const fn continuation(&self) -> &InvocationContinuation {
        &self.continuation
    }

    /// Checked target root/ASID/table metadata to install after guards end.
    pub const fn translation(&self) -> PreparedTranslationContext {
        self.translation
    }

    /// Target entry PC: the Invocation's mandatory function address.
    pub const fn entry_pc(&self) -> NonZero<u64> {
        self.entry_pc
    }

    /// Target SP consumed from saved `x9` and validated against the extent.
    pub const fn target_sp(&self) -> u64 {
        self.target_sp
    }

    /// The six real Call inputs from saved `x2..x7`, unchanged.
    pub const fn arguments(&self) -> [u64; 6] {
        self.arguments
    }

    /// The key the callee receives in `x0` for a `CallWithKey`: its borrowed
    /// entry's callee-local packed key. `None` for a plain Call.
    pub fn placed_key(&self) -> Option<RawKey> {
        self.placement.map(|placement| placement.placed_key)
    }

    /// Saved-continuation count before the push this Call would perform.
    pub const fn depth(&self) -> usize {
        self.depth
    }
}

impl<A: ArchObjects> Nucleus<A> {
    /// The key-resolution context of `address_space`: its carved table's
    /// address and guard, the latter from the `SELF_KEYTABLE` capability,
    /// validated to name this very table. `invoked` is the key reported if the
    /// context cannot be established.
    pub(crate) fn address_space_table(
        access: &Access,
        address_space: AddressSpaceId,
        invoked: RawKey,
    ) -> Result<CallerTable, CapError> {
        let binding = access
            .resolve_object::<A::AddressSpace>(address_space)
            .map_err(|_invalid_address_space| CapError::InvalidDomain)?
            .keytable()
            .ok_or(CapError::NotBound)?;
        let no_context = || CapError::InvalidSelfReference {
            reference: InvalidSelfReference::KeyTableKey(KeySlot::SELF_KEYTABLE),
            key: invoked,
        };
        // A retired table fails every invocation from the AddressSpace bound
        // to it, reported against the invoked key.
        let table = access
            .resolve_object::<KeyTable>(binding.id())
            .map_err(|error| error.for_key(invoked, 0))?;
        let (named, guard, size_bits) = table.self_table_capability().ok_or_else(no_context)?;
        if named != binding.id()
            || size_bits != binding.size_bits()
            || table.size_bits() != binding.size_bits()
        {
            return Err(no_context());
        }
        Ok(CallerTable {
            table: binding.id(),
            guard,
        })
    }

    /// Admit and prepare `Invocation.Call` (or, with `lend`,
    /// `Invocation.CallWithKey`) for the current Thread, without committing
    /// anything.
    ///
    /// `saved` is the caller's saved exception frame: the target SP is read
    /// from saved `x9` (provisional transport), never the live register, and
    /// the six Call inputs from saved `x2..x7`. Stage order follows the
    /// selected admission order after key/operation/authority checks:
    ///
    /// 1. live target `AddressSpace` identity (`InconsistentKey`/`ObjectRetired`
    ///    on the Invocation key in `x0`);
    /// 2. `CallWithKey` only: the Invocation accepts keys (`KeyNotAccepted`);
    /// 3. `CallWithKey` only: the lent key resolves through the caller's table
    ///    (reported against operand `x8`), is a lendable kind
    ///    (`InvalidObjectType`) and carries `LEND` (`InsufficientRights`);
    /// 4. ordered SP predicates (`InvalidStack`);
    /// 5. target keytable binding (`NotBound`) and translation-context
    ///    readiness/encodability;
    /// 6. invocation-stack capacity (`NestingDepth { count }`);
    /// 7. `CallWithKey` only: a vacant placement slot in the target's table
    ///    (`NoPlacementSlot`).
    ///
    /// Taking `&self` makes non-mutation structural: every rejection preserves
    /// the source Thread, its invocation stack, pending records, scheduling
    /// state, translation hardware and capability tables.
    pub fn prepare_call(
        &self,
        access: &Access,
        saved: &SavedContext,
        target: CallTarget,
        lend: Option<LendRequest>,
    ) -> Result<PreparedCall, CapError> {
        if !execution_origin(saved.origin) {
            return Err(CapError::InvalidDomain);
        }
        let source_thread = self.current_thread_id()?;
        let source = self.current_thread_ref()?;
        if source.context != ExecutionContext::Running {
            return Err(CapError::InvalidOperation);
        }
        let invocation_key = RawKey::from_wire(saved.gpr[0]);

        // Stage 1: stale or reused target identity precedes supplied values.
        access
            .resolve_object::<A::AddressSpace>(target.address_space)
            .map_err(|error| error.for_key(invocation_key, 0))?;

        // Stages 2 and 3: the exporter accepts keys, and the lent key is
        // resolvable and lendable.
        if let Some(lend) = lend {
            Self::admit_lent_key(access, target.placement_window, lend)?;
        }

        // Stage 4: the submitted SP, from the saved frame only.
        let target_sp = saved.gpr[9];
        target.stack_extent.validate_sp(target_sp)?;

        // Stage 5: keytable binding, root/ASID readiness and backend
        // encodability.
        let translation = prepare_translation_context::<A>(access, target.address_space)?;

        // Stage 6: a full stack reports the current count, never depth + 1.
        let depth = source.invocation_stack.len();
        if source.invocation_stack.is_full() {
            return Err(CapError::NestingDepth {
                count: u64::try_from(depth).map_err(|_too_wide| CapError::InvalidOperation)?,
            });
        }

        // Stage 7: the placement slot, with the exact key the callee receives.
        let placement = lend
            .map(|lend| Self::prepare_placement(access, target, invocation_key, lend))
            .transpose()?;

        let mut arguments = [0; 6];
        arguments.copy_from_slice(&saved.gpr[2..8]);
        Ok(PreparedCall {
            source_thread,
            continuation: InvocationContinuation::from_saved(
                source.address_space,
                *saved,
                self.current_time_ns(),
            ),
            translation,
            entry_pc: target.function_address,
            target_sp,
            arguments,
            placement,
            depth,
        })
    }

    /// Stages 2 and 3 of `CallWithKey` admission.
    fn admit_lent_key(
        access: &Access,
        window: PlacementWindow,
        lend: LendRequest,
    ) -> Result<(), CapError> {
        if !window.accepts_keys() {
            return Err(CapError::KeyNotAccepted);
        }
        let caller_table = access.resolve_caller_table(lend.caller)?;
        let entry = caller_table
            .lookup(lend.key, lend.caller.guard)
            .map_err(|error| error.with_key_operand(LENT_KEY_OPERAND))?;
        if !KeyEntry::is_derivable_kind(entry.object_type()) {
            return Err(CapError::InvalidObjectType(entry.object_type()));
        }
        if !entry.rights().has(Rights::LEND) {
            return Err(CapError::InsufficientRights);
        }
        Ok(())
    }

    /// Stage 7 of `CallWithKey` admission: the first vacant slot of the
    /// exporter's placement window in the target's table, and the key the
    /// installation will issue there.
    ///
    /// A target table that does not name itself is the exporter's internal
    /// state: the caller holds only `CALL` on it, so it learns that the
    /// target cannot receive keys (`NotBound`), not why.
    fn prepare_placement(
        access: &Access,
        target: CallTarget,
        invocation_key: RawKey,
        lent: LendRequest,
    ) -> Result<PreparedPlacement, CapError> {
        let target_table = Self::address_space_table(access, target.address_space, invocation_key)
            .map_err(|error| match error {
                CapError::InvalidSelfReference { .. } => CapError::NotBound,
                other => other,
            })?;
        let table = access.resolve_caller_table(target_table)?;
        let (slot, incarnation) = table
            .first_vacant_in(target.placement_window)
            .ok_or(CapError::NoPlacementSlot)?;
        Ok(PreparedPlacement {
            lent,
            target_table,
            slot,
            incarnation,
            placed_key: RawKey::from_parts(
                target_table.guard,
                table.size_bits(),
                slot.0,
                incarnation,
            ),
        })
    }
}

/// SPSR condition flags N, Z, C and V (bits 31..28).
const SPSR_NZCV: u64 = 0xF000_0000;

impl PreparedCall {
    /// The scrubbed target-entry execution frame for this Call.
    ///
    /// Only the six real inputs survive, unchanged in `x2..x7`; dummy `x0`/`x1`
    /// and `x8..x30` (including the consumed `x9` and `x18`) are zero. PC is the
    /// Invocation entry, SP the validated target SP. SPSR inherits the saved
    /// source mode, masks and other controls with only NZCV cleared; the
    /// exception origin is unchanged, so Call never changes privilege. TLS
    /// (`TPIDR_EL0`) is zero: the source's value is saved in the continuation.
    pub fn target_entry_context(&self) -> SavedContext {
        let mut gpr = [0; 30];
        gpr[2..8].copy_from_slice(&self.arguments);
        if let Some(placement) = self.placement {
            gpr[0] = placement.placed_key.to_wire();
        }
        SavedContext {
            gpr,
            lr: 0,
            spsr_el1: self.continuation.source_spsr & !SPSR_NZCV,
            elr_el1: self.entry_pc.get(),
            sp: self.target_sp,
            origin: self.continuation.source_origin,
            tpidr_el0: 0,
        }
    }
}

/// A committed Call whose hardware installation/frame rewrite is still owed.
///
/// No borrowed Thread, `AddressSpace`, table or trap frame survives. Entry
/// installs `translation` after all guards and the kernel lock end, restores
/// `target` into the transient frame, and only then emits the success trace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommittedCall {
    pub source_thread: ThreadId,
    pub target: SavedContext,
    pub translation: PreparedTranslationContext,
    /// The call lent a key (`CallWithKey`); entry traces it as such.
    pub with_key: bool,
}

impl<A: ArchObjects> Nucleus<A> {
    /// Commit a prepared Call: install the borrowed entry of a `CallWithKey`,
    /// push the source continuation and migrate the current Thread into the
    /// target `AddressSpace` (and thus its table).
    ///
    /// `prepared` must come from [`Nucleus::prepare_call`] in the same
    /// serialized trap interval. Its cheap invariants are re-checked first so
    /// a stale description is rejected with `InvalidOperation` before any
    /// mutation: same current Thread incarnation, still running, still in the
    /// captured source `AddressSpace`, and the stack depth it was admitted at.
    /// The target translation is not re-resolved: no transaction can retire
    /// or rebind it inside the interval. No hardware is touched here.
    ///
    /// The borrowed entry is installed before the push, into the slot admission
    /// found vacant, and removed again if the push fails, so a failure leaves
    /// both tables unchanged.
    pub fn commit_call(
        &mut self,
        access: &Access,
        prepared: PreparedCall,
    ) -> Result<CommittedCall, CapError> {
        let current = self.current_thread.ok_or(CapError::InvalidDomain)?;
        let source_thread = prepared.source_thread;
        if source_thread != current {
            return Err(CapError::InvalidOperation);
        }
        {
            let source = self
                .thread_mut(source_thread)
                .ok_or(CapError::InvalidOperation)?;
            if source.context != ExecutionContext::Running
                || source.address_space != prepared.continuation.source_address_space
                || source.invocation_stack.len() != prepared.depth
            {
                return Err(CapError::InvalidOperation);
            }
        }

        // Commit. The borrowed entry goes in first; the push is the last
        // fallible step and leaves the stack unchanged on failure.
        let mut continuation = prepared.continuation;
        if let Some(placement) = prepared.placement {
            continuation.placement = Self::install_placement(access, placement)?;
        }
        let source = self
            .thread_mut(source_thread)
            .ok_or(CapError::InvalidOperation)?;
        if source.invocation_stack.push(continuation).is_err() {
            Self::release_placement(
                access,
                prepared
                    .placement
                    .map_or(CarvedId::UNBOUND, |placement| placement.target_table.table),
                continuation.placement,
            );
            return Err(CapError::NestingDepth {
                count: u64::try_from(prepared.depth).unwrap_or(u64::MAX),
            });
        }
        source.address_space = prepared.translation.address_space();
        Ok(CommittedCall {
            source_thread,
            target: prepared.target_entry_context(),
            translation: prepared.translation,
            with_key: prepared.placement.is_some(),
        })
    }

    /// Install the borrowed entry of a lent key into its admitted slot.
    ///
    /// The lent entry is looked up again through the caller's table (nothing
    /// interleaves since admission) and copied with its exact rights, marked
    /// `LENT`. Returns where it was placed, for the continuation record.
    fn install_placement(
        access: &Access,
        placement: PreparedPlacement,
    ) -> Result<Placement, CapError> {
        let borrowed = {
            let caller_table = access.resolve_caller_table(placement.lent.caller)?;
            caller_table
                .lookup(placement.lent.key, placement.lent.caller.guard)
                .map_err(|error| error.with_key_operand(LENT_KEY_OPERAND))?
                .borrowed_copy()
        };
        let mut target_table = access.resolve_caller_table(placement.target_table)?;
        let placed = target_table
            .insert(placement.slot, borrowed, placement.target_table.guard)
            .map_err(|failure| failure.error)?;
        let installed = Placement {
            slot: placement.slot.0,
            incarnation: placement.incarnation,
        };
        if placed != placement.placed_key {
            // Admission computed this key from the same table state, so the
            // issued key must match; undo rather than hand out a stranger.
            target_table.release_placed(placement.slot, placement.incarnation);
            return Err(CapError::InvalidOperation);
        }
        Ok(installed)
    }

    /// Remove the borrowed entry `placement` names from `table`, if the table
    /// is live and it is still the entry issued then. Called when a
    /// call that lent a key returns, and when its Thread stops for good.
    /// Returns whether an entry was removed; a callee that already deleted it
    /// leaves nothing to do.
    pub fn release_placement(access: &Access, table: CarvedId, placement: Placement) -> bool {
        if !placement.is_some() {
            return false;
        }
        let Ok(mut table) = access.resolve_object::<KeyTable>(table) else {
            return false;
        };
        table.release_placed(KeySlot(placement.slot), placement.incarnation)
    }

    /// Remove every borrowed entry the outstanding calls of `thread` placed.
    ///
    /// The callee of the call recorded at depth `i` is the source of the record
    /// at depth `i + 1`, or the Thread's current `AddressSpace` for the top
    /// record. A callee whose `AddressSpace` is gone took its table with it, so
    /// there is nothing to remove. Called when the Thread stops for good
    /// (retired, or parked as faulted).
    pub fn release_thread_placements(&self, access: &Access, thread: ThreadId) {
        let Some(thread) = self.thread(thread) else {
            return;
        };
        let records = thread.invocation_stack.records();
        for (depth, record) in records.iter().enumerate() {
            if !record.placement.is_some() {
                continue;
            }
            let callee = records
                .get(depth + 1)
                .map_or(thread.address_space, |next| next.source_address_space);
            if let Ok(table) = Self::address_space_table(access, callee, RawKey::from_wire(0)) {
                Self::release_placement(access, table.table, record.placement);
            }
        }
    }
}

// ═════════════════════════════
// THREAD.RETURN
// ═════════════════════════════

/// Return protocol faults, classified here and delivered by entry.
///
/// Neither is an ordinary recoverable error and neither pops the stack; they
/// never reach the wire. Entry delivers them to the fault handler of the
/// `AddressSpace` the Thread executes in, as a fault at the Return's `svc`
/// (see `objects::fault`). In trusted `EL1t` code they halt the kernel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReturnFault {
    /// Depth-zero underflow: no continuation to return to.
    IllegalReturn,
    /// The top record's source `AddressSpace` is no longer live.
    ReturnTargetRetired,
}

/// Why a `Thread.Return` was not admitted.
pub enum ReturnRejection {
    /// Ordinary pre-commit rejection (lookup, form, kernel invariants);
    /// recoverable through the shared status/detail encoding.
    Error(CapError),
    /// Protocol fault for the Thread's fault handler; never a wire error.
    Fault(ReturnFault),
}

impl From<CapError> for ReturnRejection {
    fn from(error: CapError) -> Self {
        Self::Error(error)
    }
}

/// A fully admitted, uncommitted `Thread.Return` description.
///
/// Producing this mutates nothing. Valid only within the serialized trap
/// interval that produced it, like [`PreparedCall`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreparedReturn {
    source_thread: ThreadId,
    current_address_space: AddressSpaceId,
    continuation: InvocationContinuation,
    translation: PreparedTranslationContext,
    payload: [u64; 2],
    depth: usize,
}

impl PreparedReturn {
    pub const fn source_thread(&self) -> ThreadId {
        self.source_thread
    }

    /// The top record this Return would pop.
    pub const fn continuation(&self) -> &InvocationContinuation {
        &self.continuation
    }

    /// Checked source root/ASID/table metadata to install after guards end.
    pub const fn translation(&self) -> PreparedTranslationContext {
        self.translation
    }

    /// Target-provided result words `(r0, r1)` from saved `x2`/`x3`.
    pub const fn payload(&self) -> [u64; 2] {
        self.payload
    }

    /// Saved-continuation count before the pop.
    pub const fn depth(&self) -> usize {
        self.depth
    }

    /// The resumed source frame: `x0 = SUCCESS`, `x1 = r0`, `x2 = r1`,
    /// `x3..x18` zero, exact saved `x19..x30`, SP, PC, origin, raw SPSR
    /// (including the source's original NZCV) and TLS (`TPIDR_EL0`).
    pub fn source_resume_context(&self) -> SavedContext {
        let record = &self.continuation;
        let mut gpr = [0; 30];
        gpr[0] = libobject::syscall_status::SUCCESS;
        gpr[1] = self.payload[0];
        gpr[2] = self.payload[1];
        gpr[19..30].copy_from_slice(&record.source_x19_x30[..11]);
        SavedContext {
            gpr,
            lr: record.source_x19_x30[11],
            spsr_el1: record.source_spsr,
            elr_el1: record.source_pc,
            sp: record.source_sp,
            origin: record.source_origin,
            tpidr_el0: record.source_tpidr_el0,
        }
    }
}

/// A committed Return whose hardware installation/frame rewrite is still owed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommittedReturn {
    pub source_thread: ThreadId,
    pub resumed: SavedContext,
    pub translation: PreparedTranslationContext,
    /// The Return ended a fault handler with the terminate action: instead of
    /// resuming, entry parks the Thread as faulted with `resumed` (its
    /// faulting state).
    pub terminate: bool,
    /// The borrowed entry the popped call placed in this (the callee's) table,
    /// still to be removed by the API layer after the pop; `Placement::NONE`
    /// for a plain Call or a fault handler's Return.
    pub placement: Placement,
}

impl<A: ArchObjects> Nucleus<A> {
    /// Admit and prepare `Thread.Return` for the current Thread, after the API
    /// layer resolved the `CurrentReturnOnly` key. Mutates nothing.
    ///
    /// Payload `r0`/`r1` is read from saved `x2`/`x3`; `x4..x7` are ignored.
    /// Order: kernel invariants (`Error`), then underflow
    /// (`Fault(IllegalReturn)`), then the top record's source identity
    /// (`Fault(ReturnTargetRetired)`), then its translation readiness.
    ///
    /// Implementation status: a live source `AddressSpace` whose root/ASID is
    /// missing or unencodable is not classified by the contract; it surfaces as
    /// the preparation `Error` without a pop until a decision covers it.
    pub fn prepare_return(
        &self,
        access: &Access,
        saved: &SavedContext,
    ) -> Result<PreparedReturn, ReturnRejection> {
        if !execution_origin(saved.origin) {
            return Err(CapError::InvalidDomain.into());
        }
        let source_thread = self.current_thread_id()?;
        let thread = self.current_thread_ref()?;
        if thread.context != ExecutionContext::Running {
            return Err(CapError::InvalidOperation.into());
        }

        let continuation = *thread
            .invocation_stack
            .top()
            .ok_or(ReturnRejection::Fault(ReturnFault::IllegalReturn))?;
        access
            .resolve_object::<A::AddressSpace>(continuation.source_address_space)
            .map_err(|_retired| ReturnRejection::Fault(ReturnFault::ReturnTargetRetired))?;
        let translation =
            prepare_translation_context::<A>(access, continuation.source_address_space)?;

        Ok(PreparedReturn {
            source_thread,
            current_address_space: thread.address_space,
            continuation,
            translation,
            payload: [saved.gpr[2], saved.gpr[3]],
            depth: thread.invocation_stack.len(),
        })
    }

    /// Commit a prepared Return: pop the invoking Thread's own top record and
    /// migrate it back into the source `AddressSpace` (and thus its table).
    ///
    /// A stale description (different current Thread incarnation, not
    /// running, moved `AddressSpace`, changed depth or top record) is rejected
    /// with `InvalidOperation` before any mutation. No hardware is touched.
    /// Per-call time attribution from the record's stamp stays inert until
    /// the Time subsystem exists.
    pub fn commit_return(&mut self, prepared: PreparedReturn) -> Result<CommittedReturn, CapError> {
        let current = self.current_thread.ok_or(CapError::InvalidDomain)?;
        let source_thread = prepared.source_thread;
        if source_thread != current {
            return Err(CapError::InvalidOperation);
        }
        let thread = self
            .thread_mut(source_thread)
            .ok_or(CapError::InvalidOperation)?;
        if thread.context != ExecutionContext::Running
            || thread.address_space != prepared.current_address_space
            || thread.invocation_stack.len() != prepared.depth
            || thread.invocation_stack.top() != Some(&prepared.continuation)
        {
            return Err(CapError::InvalidOperation);
        }

        // Commit: the checked top is popped and the Thread migrates back.
        let popped = thread.invocation_stack.pop();
        debug_assert_eq!(popped, Some(prepared.continuation));
        thread.address_space = prepared.continuation.source_address_space;

        // Popping the fault continuation ends the fault handler: resume from
        // the fault frame as the handler's action selects, and free it.
        let handled_fault = thread.fault.filter(|fault| fault.depth == prepared.depth);
        let Some(fault) = handled_fault else {
            return Ok(CommittedReturn {
                source_thread,
                resumed: prepared.source_resume_context(),
                translation: prepared.translation,
                terminate: false,
                placement: prepared.continuation.placement,
            });
        };
        thread.fault = None;
        let mut resumed = fault.frame;
        let action = FaultAction::from_wire(prepared.payload[0]);
        if action == FaultAction::Skip {
            // Every A64 instruction is four bytes.
            resumed.elr_el1 = resumed.elr_el1.wrapping_add(4);
        }
        self.set_fault_handler_busy(fault.address_space, false);
        Ok(CommittedReturn {
            source_thread,
            resumed,
            translation: prepared.translation,
            terminate: action == FaultAction::Terminate,
            placement: prepared.continuation.placement,
        })
    }
}
