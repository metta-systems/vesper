//! Objects carved by `Untyped.Retype` from the invoking Untyped's memory.
//!
//! A carved object is laid out as a [`Carved<T>`]: a [`CarvedHeader`] holding
//! the object's kind, its state (live or retired) and a generation, followed
//! by the object itself. The kernel names it by [`CarvedId`] — kind, kernel
//! address and generation — in capability payloads, pending records and Time
//! chains. Resolution (`Access::resolve_object`) reads the header and checks
//! all three before handing out a guard.
//!
//! The slot incarnation matches a component's key to its `KeyTable` entry; the
//! generation matches that entry to the object. Revoke and Retire bump the
//! generation ([`Carved::retire`]) before the userspace cleanup manager
//! deletes the stale entries, so every capability still naming the object
//! fails at once. Memory is re-carved only after that cleanup, so a fresh
//! header starts at [`FIRST_GENERATION`] again.

use {
    crate::objects::{NucleusObject, object_pool::IdentityError},
    core::mem::{align_of, size_of},
    libobject::ObjectType,
};

/// A carved object that is live.
const STATE_LIVE: u8 = 1;
/// A carved object that has been retired: every identity naming it fails.
const STATE_RETIRED: u8 = 2;

/// The generation of a freshly carved object.
pub const FIRST_GENERATION: u32 = 1;

/// Kernel header at the start of every carved object.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct CarvedHeader {
    kind: ObjectType,
    state: u8,
    _reserved: u16,
    generation: u32,
}

/// A carved object: its header followed by the object.
#[repr(C)]
pub struct Carved<T> {
    header: CarvedHeader,
    object: T,
}

/// Kernel identity of a carved object: its kind, the kernel address of its
/// [`Carved`] record, and the generation it was carved with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CarvedId {
    pub kind: ObjectType,
    pub address: u64,
    pub generation: u32,
}

impl CarvedId {
    /// Names no object: an inert Thread's `AddressSpace` before
    /// `Scheduler.ConfigureThread`. Its address is zero, so validating it
    /// always fails.
    pub const UNBOUND: Self = Self {
        kind: ObjectType::NULL,
        address: 0,
        generation: 0,
    };
}

/// Object kinds the kernel carves from Untyped memory.
pub trait CarvedObject: NucleusObject {}

impl<T: CarvedObject> Carved<T> {
    /// Bytes one carved object takes from an Untyped.
    pub const SIZE: usize = size_of::<Self>();
    /// Alignment of one carved object.
    pub const ALIGN: usize = align_of::<Self>();

    /// Write a live `object` with a fresh header at `address` and return its
    /// identity.
    ///
    /// # Safety
    /// `address` must be a kernel-writable, exclusively owned region of at
    /// least [`Self::SIZE`] bytes aligned to [`Self::ALIGN`] that nothing
    /// else names.
    pub unsafe fn initialize(address: *mut u8, object: T) -> CarvedId {
        let record = address.cast::<Self>();
        // SAFETY: the caller guarantees an exclusive, aligned, large enough
        // region.
        unsafe {
            record.write(Self {
                header: CarvedHeader {
                    kind: T::TYPE,
                    state: STATE_LIVE,
                    _reserved: 0,
                    generation: FIRST_GENERATION,
                },
                object,
            });
        }
        CarvedId {
            kind: T::TYPE,
            address: address as u64,
            generation: FIRST_GENERATION,
        }
    }

    /// Check `id` against the header at its address and return a pointer to
    /// the live object.
    ///
    /// # Safety
    /// `id.address` must have been issued by [`Self::initialize`] for a
    /// region that is still mapped and never handed out as anything else.
    pub unsafe fn validate(id: CarvedId) -> Result<*mut T, IdentityError> {
        if id.kind != T::TYPE || id.address == 0 || !id.address.is_multiple_of(Self::ALIGN as u64) {
            return Err(IdentityError::Malformed);
        }
        let record = id.address as *mut Self;
        // SAFETY: the caller guarantees the address names a carved record.
        let header = unsafe { (*record).header };
        if header.kind != T::TYPE {
            return Err(IdentityError::Malformed);
        }
        if header.state != STATE_LIVE || header.generation != id.generation {
            return Err(IdentityError::Retired);
        }
        // SAFETY: the record is live; the object follows its header.
        Ok(unsafe { &raw mut (*record).object })
    }

    /// Retire the object at `id`: bump its generation and mark it retired,
    /// so every capability still naming the old generation fails before the
    /// userspace cleanup manager deletes those entries. A generation already
    /// at its maximum stays there; the retired state alone then keeps the
    /// object unusable.
    ///
    /// # Safety
    /// As [`Self::validate`]; no guard to the object may be live.
    pub unsafe fn retire(id: CarvedId) -> Result<(), IdentityError> {
        // SAFETY: forwarded caller guarantee.
        unsafe {
            Self::validate(id)?;
        }
        let record = id.address as *mut Self;
        // SAFETY: validated above.
        unsafe {
            let header = &mut (*record).header;
            header.generation = header.generation.saturating_add(1);
            header.state = STATE_RETIRED;
        }
        Ok(())
    }

    /// The generation currently recorded in the header at `id.address`.
    ///
    /// # Safety
    /// As [`Self::validate`].
    #[cfg(test)]
    unsafe fn generation(id: CarvedId) -> u32 {
        // SAFETY: forwarded caller guarantee.
        unsafe { (*(id.address as *const Self)).header.generation }
    }
}

/// A structural identity for unit tests that never resolve it.
#[cfg(test)]
pub const fn test_id(kind: ObjectType, address: u64) -> CarvedId {
    CarvedId {
        kind,
        address,
        generation: FIRST_GENERATION,
    }
}

#[cfg(test)]
mod tests {
    use {
        super::{Carved, CarvedId, FIRST_GENERATION},
        crate::objects::{EventCount, Notification, object_pool::IdentityError},
        libobject::ObjectType,
    };

    #[repr(C, align(64))]
    struct Backing([u8; 512]);

    #[test_case]
    fn carved_identity_validates_kind_state_and_generation() {
        let mut backing = Backing([0; 512]);
        let address = backing.0.as_mut_ptr();
        // SAFETY: the backing is exclusively owned, aligned and large enough.
        let id = unsafe { Carved::<Notification>::initialize(address, Notification::new()) };
        assert_eq!(id.kind, ObjectType::NOTIFICATION);
        assert_eq!(id.generation, FIRST_GENERATION);
        // SAFETY: the id was issued for this backing.
        assert!(unsafe { Carved::<Notification>::validate(id) }.is_ok());

        let stale = CarvedId {
            generation: FIRST_GENERATION + 1,
            ..id
        };
        // SAFETY: as above.
        assert_eq!(
            unsafe { Carved::<Notification>::validate(stale) }.err(),
            Some(IdentityError::Retired)
        );
        // The same address under another kind is malformed.
        let wrong_kind = CarvedId {
            kind: ObjectType::EVENT_COUNT,
            ..id
        };
        // SAFETY: as above.
        assert_eq!(
            unsafe { Carved::<EventCount>::validate(wrong_kind) }.err(),
            Some(IdentityError::Malformed)
        );

        // SAFETY: as above; no guard is live.
        unsafe { Carved::<Notification>::retire(id) }.ok().unwrap();
        // Retirement bumps the generation, so the old identity no longer
        // matches, and the retired state keeps the new one unusable too.
        // SAFETY: as above.
        assert_eq!(
            unsafe { Carved::<Notification>::generation(id) },
            FIRST_GENERATION + 1
        );
        // SAFETY: as above.
        assert_eq!(
            unsafe { Carved::<Notification>::validate(id) }.err(),
            Some(IdentityError::Retired)
        );
        let bumped = CarvedId {
            generation: FIRST_GENERATION + 1,
            ..id
        };
        // SAFETY: as above.
        assert_eq!(
            unsafe { Carved::<Notification>::validate(bumped) }.err(),
            Some(IdentityError::Retired)
        );
        // Retiring again through the stale identity fails.
        // SAFETY: as above.
        assert_eq!(
            unsafe { Carved::<Notification>::retire(id) }.err(),
            Some(IdentityError::Retired)
        );
    }
}
