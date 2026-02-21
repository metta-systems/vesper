// SPDX-License-Identifier: MIT OR Apache-2.0
//
// Copyright (c) 2020-2022 Andre Richter <andre.o.richter@gmail.com>

//! A record of mapped pages.

use {
    super::{AccessPermissions, AttributeFields, MMIODescriptor, MemAttributes, MemoryRegion},
    libaddress::{Address, Physical, Virtual},
    liblog::info,
};

//--------------------------------------------------------------------------------------------------
// Private Definitions
//--------------------------------------------------------------------------------------------------

/// Type describing a virtual memory mapping.
#[allow(missing_docs, dead_code)]
#[derive(Copy, Clone)]
struct MappingRecordEntry<const PAGE_SIZE: usize> {
    pub users: [Option<&'static str>; 5],
    pub phys_start_addr: Address<Physical>,
    pub virt_start_addr: Address<Virtual>,
    pub num_pages: usize,
    pub attribute_fields: AttributeFields,
}

#[allow(missing_docs, dead_code)]
struct MappingRecord<const PAGE_SIZE: usize> {
    inner: [Option<MappingRecordEntry<PAGE_SIZE>>; 12],
}

//--------------------------------------------------------------------------------------------------
// Global instances
//--------------------------------------------------------------------------------------------------

// FIXME: global state
// static KERNEL_MAPPING_RECORD: InitStateLock<MappingRecord<4096>> =
//     InitStateLock::new(MappingRecord::new());

//--------------------------------------------------------------------------------------------------
// Private Code
//--------------------------------------------------------------------------------------------------

impl<const PAGE_SIZE: usize> MappingRecordEntry<PAGE_SIZE> {
    #[allow(missing_docs, dead_code)]
    pub fn new(
        name: &'static str,
        virt_region: &MemoryRegion<Virtual, PAGE_SIZE>,
        phys_region: &MemoryRegion<Physical, PAGE_SIZE>,
        attr: AttributeFields,
    ) -> Self {
        Self {
            users: [Some(name), None, None, None, None],
            phys_start_addr: phys_region.start_addr(),
            virt_start_addr: virt_region.start_addr(),
            num_pages: phys_region.num_pages(),
            attribute_fields: attr,
        }
    }

    #[allow(missing_docs, dead_code)]
    fn find_next_free_user(&mut self) -> Result<&mut Option<&'static str>, &'static str> {
        if let Some(x) = self.users.iter_mut().find(|x| x.is_none()) {
            return Ok(x);
        };

        Err("Storage for user info exhausted")
    }

    #[allow(missing_docs, dead_code)]
    pub fn add_user(&mut self, user: &'static str) -> Result<(), &'static str> {
        let x = self.find_next_free_user()?;
        *x = Some(user);
        Ok(())
    }
}

impl<const PAGE_SIZE: usize> MappingRecord<PAGE_SIZE> {
    #[allow(missing_docs, dead_code)]
    pub const fn new() -> Self {
        Self { inner: [None; 12] }
    }

    #[allow(missing_docs, dead_code)]
    fn size(&self) -> usize {
        self.inner.iter().filter(|x| x.is_some()).count()
    }

    #[allow(missing_docs, dead_code)]
    fn sort(&mut self) {
        let upper_bound_exclusive = self.size();
        let entries = &mut self.inner.get_mut(0..upper_bound_exclusive).unwrap();

        if !entries.is_sorted_by_key(|item| item.unwrap().virt_start_addr) {
            entries.sort_unstable_by_key(|item| item.unwrap().virt_start_addr);
        }
    }

    #[allow(missing_docs, dead_code)]
    fn find_next_free(
        &mut self,
    ) -> Result<&mut Option<MappingRecordEntry<PAGE_SIZE>>, &'static str> {
        if let Some(x) = self.inner.iter_mut().find(|x| x.is_none()) {
            return Ok(x);
        }

        Err("Storage for mapping info exhausted")
    }

    #[allow(missing_docs, dead_code)]
    fn find_duplicate(
        &mut self,
        phys_region: &MemoryRegion<Physical, PAGE_SIZE>,
    ) -> Option<&mut MappingRecordEntry<PAGE_SIZE>> {
        self.inner
            .iter_mut()
            .filter_map(|x| x.as_mut())
            .filter(|x| x.attribute_fields.mem_attributes == MemAttributes::Device)
            .find(|x| {
                x.phys_start_addr == phys_region.start_addr()
                    && x.num_pages == phys_region.num_pages()
            })
    }

    /// Adds a new mapping to the mapping record.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the entity that owns the mapping.
    /// * `virt_region` - The virtual memory region being mapped.
    /// * `phys_region` - The physical memory region being mapped.
    /// * `attr` - The memory attributes of the mapping.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success, or a string error message on failure.
    #[allow(missing_docs, dead_code)]
    pub fn add(
        &mut self,
        name: &'static str,
        virt_region: &MemoryRegion<Virtual, PAGE_SIZE>,
        phys_region: &MemoryRegion<Physical, PAGE_SIZE>,
        attr: AttributeFields,
    ) -> Result<(), &'static str> {
        let x = self.find_next_free()?;

        *x = Some(MappingRecordEntry::new(
            name,
            virt_region,
            phys_region,
            attr,
        ));

        self.sort();

        Ok(())
    }

    #[allow(missing_docs, dead_code)]
    pub fn print(&self) {
        info!(
            "      -------------------------------------------------------------------------------------------------------------------------------------------"
        );
        info!(
            "      {:^44}     {:^30}   {:^7}   {:^9}   {:^35}",
            "Virtual", "Physical", "Size", "Attr", "Entity"
        );
        info!(
            "      -------------------------------------------------------------------------------------------------------------------------------------------"
        );

        for i in self.inner.iter().flatten() {
            let size = i.num_pages * PAGE_SIZE;
            let virt_start = i.virt_start_addr;
            let virt_end_inclusive = virt_start + (size - 1);
            let phys_start = i.phys_start_addr;
            let phys_end_inclusive = phys_start + (size - 1);

            let (size, unit) = liblog::size_human_readable_ceil(size);

            let attr = match i.attribute_fields.mem_attributes {
                MemAttributes::CacheableDRAM => "C",
                MemAttributes::NonCacheableDRAM => "NC",
                MemAttributes::Device => "Dev",
            };

            let acc_p = match i.attribute_fields.acc_perms {
                AccessPermissions::ReadOnly => "RO",
                AccessPermissions::ReadWrite => "RW",
            };

            let xn = if i.attribute_fields.executable {
                "X"
            } else {
                "XN"
            };

            info!(
                "      {}..{} --> {}..{} | {:>3} {} | {:<3} {} {:<2} | {}",
                virt_start,
                virt_end_inclusive,
                phys_start,
                phys_end_inclusive,
                size,
                unit,
                attr,
                acc_p,
                xn,
                i.users[0].unwrap()
            );

            for k in &i.users[1..] {
                if let Some(additional_user) = *k {
                    info!(
                        "                                                                                                            | {additional_user}",
                    );
                }
            }
        }

        info!(
            "      -------------------------------------------------------------------------------------------------------------------------------------------"
        );
    }
}

//--------------------------------------------------------------------------------------------------
// Public Code
//--------------------------------------------------------------------------------------------------

/// Add an entry to the mapping info record.
#[expect(dead_code, clippy::unnecessary_wraps)]
pub fn kernel_add<const PAGE_SIZE: usize>(
    _name: &'static str,
    _virt_region: &MemoryRegion<Virtual, PAGE_SIZE>,
    _phys_region: &MemoryRegion<Physical, PAGE_SIZE>,
    _attr: AttributeFields,
) -> Result<(), &'static str> {
    // KERNEL_MAPPING_RECORD.write(|mr| mr.add(name, virt_region, phys_region, attr))
    Ok(())
}

#[expect(dead_code, clippy::unnecessary_wraps)]
pub fn kernel_find_and_insert_mmio_duplicate<const PAGE_SIZE: usize>(
    _mmio_descriptor: &MMIODescriptor,
    _new_user: &'static str,
) -> Option<Address<Virtual>> {
    // let phys_region: MemoryRegion<Physical, PAGE_SIZE> = (*mmio_descriptor).into();

    // KERNEL_MAPPING_RECORD.write(|mr| {
    //     let dup = mr.find_duplicate(&phys_region)?;

    //     if let Err(x) = dup.add_user(new_user) {
    //         warn!("{x}");
    //     }

    //     Some(dup.virt_start_addr)
    // })
    Some(Address::zero())
}

/// Human-readable print of all recorded kernel mappings.
#[allow(missing_docs, dead_code)]
pub fn kernel_print() {
    // KERNEL_MAPPING_RECORD.read(MappingRecord::print);
}
