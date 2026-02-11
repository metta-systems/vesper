// The key insight: an Untyped capability (in keytable) IS the kernel object.
// There's no separate in-kernel structure — just the capability itself carrying all the metadata.
// The UntypedPayload is stored inline in the KeyEntry union — no pool allocation, no indirection.
//
// ┌─────────────────────────────────────────────────────────────────────┐
// │                    Untyped capability (inline in KeyEntry)          │
// ├──────────────────┬──────────────────────────────────────────────────┤
// │  obj_type        │ = ObjectType::Untyped (in KeyEntry header)      │
// ├──────────────────┼──────────────────────────────────────────────────┤
// │  rights          │ Access rights (in KeyEntry header)              │
// ├──────────────────┼──────────────────────────────────────────────────┤
// │  paddr           │ Physical address of untyped region              │
// ├──────────────────┼──────────────────────────────────────────────────┤
// │  size_bits       │ Total size in bits (region = 2^size_bits)       │
// ├──────────────────┼──────────────────────────────────────────────────┤
// │  watermark       │ Next free byte offset (>> MIN_ALIGN_BITS)       │
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
    let obj_size = if obj_type.is_core() {
        core_object_size(obj_type, size_bits)?
    } else {
        let arch_type = ArchType::try_from(obj_type)?;
        A::validate_retype(arch_type, size_bits)?
    };

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
    let entry = if obj_type.is_core() {
        create_core_object::<A>(obj_type, obj_paddr, size_bits, is_device, pools)?
    } else {
        create_arch_object::<A>(obj_type, obj_paddr, size_bits, pools)?
    };

    dest_keytable.insert(dest_slot, entry)?;

    Ok(())
}

fn create_core_object<A: ArchObjects>(
    obj_type: ObjectType,
    paddr: u64,
    size_bits: u8,
    is_device: bool,
    pools: &mut NucleusPools<A>,
) -> Result<KeyEntry, CapError> {
    match obj_type {
        ObjectType::Untyped => {
            // Split: create a child untyped — inline, no pool needed
            Ok(KeyEntry::new_untyped(
                paddr,
                size_bits,
                is_device,
                Rights::all(),
            ))
        }

        // All other core types go through pools
        // ObjectType::Notification => {
        //     let notify = Notification::new();
        //     let obj = pools.notifications.allocate(notify)
        //         .ok_or(CapError::PoolExhausted)?;
        //     Ok(KeyEntry::new(obj, Rights::all(), 0))
        // }
        //
        // ObjectType::Endpoint => {
        //     let ep = Endpoint::new();
        //     let obj = pools.endpoints.allocate(ep)
        //         .ok_or(CapError::PoolExhausted)?;
        //     Ok(KeyEntry::new(obj, Rights::all(), 0))
        // }
        //
        // ... etc for other pool-backed types
        _ => Err(CapError::InvalidObjectType),
    }
}

fn create_arch_object<A: ArchObjects>(
    obj_type: ObjectType,
    paddr: u64,
    size_bits: u8,
    pools: &mut NucleusPools<A>,
) -> Result<KeyEntry, CapError> {
    let arch_type = ArchType::try_from(obj_type)?;
    let obj_ref =
        A::create_arch_object(arch_type, PhysAddr::from(paddr), size_bits, &mut pools.arch)?;

    let rights = match obj_type {
        ObjectType::Frame => Rights::READ | Rights::WRITE,
        _ => Rights::all(),
    };

    Ok(KeyEntry::from_ref(obj_ref, rights, 0))
}

fn core_object_size(obj_type: ObjectType, size_bits: u8) -> Result<usize, CapError> {
    match obj_type {
        // ObjectType::Notification => Ok(core::mem::size_of::<Notification>()),
        // ObjectType::EventCount => Ok(core::mem::size_of::<EventCount>()),
        // ObjectType::Time => Ok(core::mem::size_of::<TimeSlice>()),
        // ObjectType::Endpoint => Ok(core::mem::size_of::<Endpoint>()),
        // ObjectType::Reply => Ok(core::mem::size_of::<Reply>()),
        // ObjectType::Domain => Ok(4096),
        // ObjectType::KeyTable => Ok(core::mem::size_of::<KeyTable>()),
        ObjectType::Buffer | ObjectType::Untyped => {
            if size_bits > 30 {
                Err(CapError::InvalidSize)
            } else {
                Ok(1usize << size_bits)
            }
        }
        _ => Err(CapError::InvalidObjectType),
    }
}
