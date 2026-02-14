// The key insight: region capabilities (Untyped, Frame) stored inline in KeyEntry
// ARE the kernel object. No separate in-kernel structure, no pool allocation,
// no pointer indirection — just the RegionPayload in the capability slot.
//
// ┌─────────────────────────────────────────────────────────────────────┐
// │                Region capabilities (inline in KeyEntry)             │
// ├──────────────────┬──────────────────────────────────────────────────┤
// │  obj_type        │ Untyped or Frame (in KeyEntry header)           │
// ├──────────────────┼──────────────────────────────────────────────────┤
// │  rights          │ Access rights (in KeyEntry header)              │
// ├──────────────────┼──────────────────────────────────────────────────┤
// │  paddr           │ Physical address of region                      │
// ├──────────────────┼──────────────────────────────────────────────────┤
// │  state           │ Untyped: watermark | Frame: map_count           │
// ├──────────────────┼──────────────────────────────────────────────────┤
// │  size_bits       │ Total size in bits (region = 2^size_bits)       │
// ├──────────────────┼──────────────────────────────────────────────────┤
// │  is_device       │ Boolean: is this device memory?                 │
// └──────────────────┴──────────────────────────────────────────────────┘

use {
    crate::{
        api::key_entry::KeyEntry,
        objects::{ArchObjects, NucleusPools, key_table::KeyTable},
    },
    libaddress::PhysAddr,
    libobject::{ArchType, CapError, KeySlot, ObjectType, Rights},
};

// ═══════════════════════════════════════════════════════════════════
// RETYPE FROM UNTYPED
// ═══════════════════════════════════════════════════════════════════
//
//                    UNTYPED REGION (2^size_bits bytes)
//      ┌────────────────────────────────────────────────────────────┐
//      │█████████████████████████░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░│
//      │      ALLOCATED          │           FREE                   │
//      └────────────────────────────────────────────────────────────┘
//      ↑                         ↑                                  ↑
//    paddr              paddr + watermark               paddr + 2^size_bits

/// Retype from an untyped KeyEntry into a new nucleus object.
///
/// `src_entry` must be an Untyped capability. The new object is allocated
/// from the untyped region and a capability is inserted into `dest_keytable`
/// at `dest_slot`.
///
/// Region types (Untyped, Frame) are created inline — no pool allocation.
/// Other arch types (PageTable, VSpace, etc.) still go through arch pools.
pub fn retype<A: ArchObjects>(
    src_entry: &mut KeyEntry,
    obj_type: ObjectType,
    size_bits: u8,
    dest_slot: KeySlot,
    dest_keytable: &mut KeyTable,
    pools: &mut NucleusPools<A>,
) -> Result<(), CapError> {
    let ut = src_entry.as_untyped_mut()?;

    // Determine object size based on type
    let obj_size = object_size::<A>(obj_type, size_bits)?;

    // Align the watermark to the object size
    let wm = ut.watermark_bytes();
    let aligned_wm = (wm + obj_size - 1) & !(obj_size - 1);

    // Check we have enough memory
    if aligned_wm + obj_size > ut.size() {
        return Err(CapError::InsufficientMemory);
    }

    let obj_paddr = ut.paddr + aligned_wm as u64;
    let is_device = ut.is_device;

    // Advance watermark
    ut.set_watermark_bytes(aligned_wm + obj_size);

    // Create the capability for the new object
    let entry = create_object::<A>(obj_type, obj_paddr, size_bits, is_device, pools)?;

    dest_keytable.insert(dest_slot, entry)?;

    Ok(())
}

/// Create a capability for a newly retyped object.
///
/// Inline region types (Untyped, Frame) are written directly into the KeyEntry.
/// Pool-backed types go through their respective allocators.
fn create_object<A: ArchObjects>(
    obj_type: ObjectType,
    paddr: u64,
    size_bits: u8,
    is_device: bool,
    pools: &mut NucleusPools<A>,
) -> Result<KeyEntry, CapError> {
    match obj_type {
        // ── Inline region types ──
        ObjectType::Untyped => Ok(KeyEntry::new_untyped(
            paddr,
            size_bits,
            is_device,
            Rights::all(),
        )),

        ObjectType::FRAME => {
            let rights = Rights::READ | Rights::WRITE;
            Ok(KeyEntry::new_frame(paddr, size_bits, is_device, rights))
        }

        // ── Pool-backed core types ──

        // ObjectType::Notification => {
        //     let obj = pools.notifications.allocate(Notification::new())
        //         .ok_or(CapError::PoolExhausted)?;
        //     Ok(KeyEntry::new(obj, Rights::all(), 0))
        // }
        // ObjectType::Endpoint => {
        //     let obj = pools.endpoints.allocate(Endpoint::new())
        //         .ok_or(CapError::PoolExhausted)?;
        //     Ok(KeyEntry::new(obj, Rights::all(), 0))
        // }
        // ... etc for other pool-backed core types

        // ── Pool-backed arch types (PageTable, VSpace, ASID, etc.) ──
        _ if obj_type.is_arch() => {
            let arch_type = ArchType::try_from(obj_type)?;
            let obj_ref = A::create_arch_object(
                arch_type,
                PhysAddr::from(paddr),
                size_bits,
                &mut pools.arch,
            )?;
            Ok(KeyEntry::from_ref(obj_ref, Rights::all(), 0))
        }

        _ => Err(CapError::InvalidObjectType),
    }
}

/// Get the physical size of an object to be created.
fn object_size<A: ArchObjects>(obj_type: ObjectType, size_bits: u8) -> Result<usize, CapError> {
    match obj_type {
        // Variable-size region types
        ObjectType::Buffer | ObjectType::Untyped => {
            if size_bits > 30 {
                Err(CapError::InvalidSize)
            } else {
                Ok(1usize << size_bits)
            }
        }

        // Frame size is validated by the arch layer
        ObjectType::FRAME => A::validate_frame_size(size_bits),

        // Fixed-size core types
        // ObjectType::Notification => Ok(core::mem::size_of::<Notification>()),
        // ObjectType::Endpoint => Ok(core::mem::size_of::<Endpoint>()),
        // ObjectType::Domain => Ok(4096),
        // ... etc

        // Arch types validated by arch layer
        _ if obj_type.is_arch() => {
            let arch_type = ArchType::try_from(obj_type)?;
            A::validate_retype(arch_type, size_bits)
        }

        _ => Err(CapError::InvalidObjectType),
    }
}
