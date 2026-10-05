//! Kernel-owned numeric stack contract carried inline in an Invocation.
//!
//! A validated extent establishes neither mapped/writable backing nor exclusive
//! stack ownership. Component setup supplies those guarantees; no allocation or
//! translation walk occurs here.
//!
//! `Nucleus::prepare_call` is the non-committing `Invocation.Call` admission
//! and preparation primitive; `Nucleus::commit_call` is its stage-5 commit.
//! Neither is wired to dispatch yet.

use {
    crate::objects::{
        ArchObjects, ExecutionContext, InvocationContinuation, Nucleus,
        access::{Access, ObjectId, PoolTag},
        resume::{PreparedTranslationContext, execution_origin, prepare_translation_context},
    },
    core::num::NonZero,
    libexception::arch::aarch64::SavedContext,
    libobject::{CapError, InvalidStackReason},
};

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
    pub address_space: ObjectId,
    pub function_address: NonZero<u64>,
    pub stack_extent: InvocationStackExtent,
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
/// Call dispatch stays unsupported until `Thread.Return` exists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreparedCall {
    source_thread: ObjectId,
    continuation: InvocationContinuation,
    translation: PreparedTranslationContext,
    entry_pc: NonZero<u64>,
    target_sp: u64,
    arguments: [u64; 6],
    depth: usize,
}

impl PreparedCall {
    /// Incarnation-checked identity of the migrating (current) Thread.
    pub const fn source_thread(&self) -> ObjectId {
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

    /// Saved-continuation count before the push this Call would perform.
    pub const fn depth(&self) -> usize {
        self.depth
    }
}

impl<A: ArchObjects> Nucleus<A> {
    /// Admit and prepare `Invocation.Call` for the current Thread, without
    /// committing anything.
    ///
    /// `saved` is the caller's saved exception frame: the target SP is read
    /// from saved `x9` (provisional transport), never the live register, and
    /// the six Call inputs from saved `x2..x7`. Stage order follows the
    /// selected admission order after key/operation/authority checks:
    ///
    /// 1. live target `AddressSpace` identity (`InvalidDomain`);
    /// 2. ordered SP predicates (`InvalidStack`);
    /// 3. target translation-context readiness/encodability;
    /// 4. invocation-stack capacity (`NestingDepth { count }`).
    ///
    /// Taking `&self` makes non-mutation structural: every rejection preserves
    /// the source Thread, its invocation stack, pending records, scheduling
    /// state, translation hardware and capability tables.
    pub fn prepare_call(
        &self,
        access: &Access,
        saved: &SavedContext,
        target: CallTarget,
    ) -> Result<PreparedCall, CapError> {
        if !execution_origin(saved.origin) {
            return Err(CapError::InvalidDomain);
        }
        let current = self.current_thread.ok_or(CapError::InvalidDomain)?;
        let current_index =
            usize::try_from(current).map_err(|_invalid_index| CapError::InvalidDomain)?;
        let source = self
            .pools
            .threads
            .get_live(current_index)
            .ok_or(CapError::InvalidDomain)?;
        if source.context != ExecutionContext::Running {
            return Err(CapError::InvalidOperation);
        }
        let source_thread = ObjectId {
            pool: PoolTag::Thread,
            index: u16::try_from(current_index).map_err(|_too_wide| CapError::InvalidDomain)?,
            generation: self
                .pools
                .threads
                .generation_of(current_index)
                .ok_or(CapError::InvalidDomain)?,
        };

        // Stage 1: stale or reused target identity precedes supplied values.
        access
            .resolve(&self.pools.arch.address_spaces, target.address_space)
            .map_err(|_invalid_identity| CapError::InvalidDomain)?;

        // Stage 2: the submitted SP, from the saved frame only.
        let target_sp = saved.gpr[9];
        target.stack_extent.validate_sp(target_sp)?;

        // Stage 3: root/ASID readiness and backend encodability.
        let translation = prepare_translation_context::<A>(
            access,
            &self.pools.arch.address_spaces,
            target.address_space,
        )?;

        // Stage 4: a full stack reports the current count, never depth + 1.
        let depth = source.invocation_stack.len();
        if source.invocation_stack.is_full() {
            return Err(CapError::NestingDepth {
                count: u64::try_from(depth).map_err(|_too_wide| CapError::InvalidOperation)?,
            });
        }

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
            depth,
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
    /// exception origin is unchanged, so Call never changes privilege.
    pub fn target_entry_context(&self) -> SavedContext {
        let mut gpr = [0; 30];
        gpr[2..8].copy_from_slice(&self.arguments);
        SavedContext {
            gpr,
            lr: 0,
            spsr_el1: self.continuation.source_spsr & !SPSR_NZCV,
            elr_el1: self.entry_pc.get(),
            sp: self.target_sp,
            origin: self.continuation.source_origin,
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
    pub source_thread: ObjectId,
    pub target: SavedContext,
    pub translation: PreparedTranslationContext,
}

impl<A: ArchObjects> Nucleus<A> {
    /// Commit a prepared Call: push the source continuation and migrate the
    /// current Thread into the target `AddressSpace` (and thus its table).
    ///
    /// `prepared` must come from [`Nucleus::prepare_call`] in the same
    /// serialized trap interval. Its cheap invariants are re-checked first so
    /// a stale description is rejected with `InvalidOperation` before any
    /// mutation: same current Thread incarnation, still running, still in the
    /// captured source `AddressSpace`, and the stack depth it was admitted at.
    /// The target translation is not re-resolved: no transaction can retire
    /// or rebind it inside the interval. No hardware is touched here.
    pub fn commit_call(&mut self, prepared: PreparedCall) -> Result<CommittedCall, CapError> {
        let current = self.current_thread.ok_or(CapError::InvalidDomain)?;
        let source_thread = prepared.source_thread;
        if u32::from(source_thread.index) != current {
            return Err(CapError::InvalidOperation);
        }
        self.pools
            .threads
            .validate(source_thread)
            .map_err(|_stale_thread| CapError::InvalidOperation)?;
        let source = self
            .pools
            .threads
            .get_live_mut(usize::from(source_thread.index))
            .ok_or(CapError::InvalidDomain)?;
        if source.context != ExecutionContext::Running
            || source.address_space != prepared.continuation.source_address_space
            || source.invocation_stack.len() != prepared.depth
        {
            return Err(CapError::InvalidOperation);
        }

        // Commit. The push is the last fallible step and leaves the stack
        // unchanged on failure; the AddressSpace switch follows it.
        source
            .invocation_stack
            .push(prepared.continuation)
            .map_err(|_full| CapError::NestingDepth {
                count: u64::try_from(prepared.depth).unwrap_or(u64::MAX),
            })?;
        source.address_space = prepared.translation.address_space();
        Ok(CommittedCall {
            source_thread,
            target: prepared.target_entry_context(),
            translation: prepared.translation,
        })
    }
}
