use {
    crate::{
        arch_trait::{EntryKind, LevelCapabilities, TranslationArch},
        error::TableError,
    },
    core::marker::PhantomData,
    libaddress::PhysAddr,
    libmapping::AttributeFields,
};

/// A single translation table at a specific level.
///
/// Wraps an externally-provided `&mut [u64]` slice — memory is never
/// allocated internally. The kernel provides table memory through its
/// capability system.
///
/// The level is stored as a runtime value so that all table capabilities
/// can be stored uniformly in the capability system without needing
/// separate types per level.
///
/// For architectures with `ENTRY_WIDTH > 1` (e.g. PowerPC HPT with 16-byte
/// PTEs), the slice contains `entries_per_table * ENTRY_WIDTH` u64 words
/// and entry access uses sub-slices of width `ENTRY_WIDTH`.
pub struct Table<'a, A: TranslationArch> {
    entries: &'a mut [u64],
    level: usize,
    _arch: PhantomData<A>,
}

/// An immutable view of a translation table.
pub struct TableRef<'a, A: TranslationArch> {
    entries: &'a [u64],
    level: usize,
    _arch: PhantomData<A>,
}

impl<'a, A: TranslationArch> Table<'a, A> {
    /// Create a new table from a zeroed memory region.
    ///
    /// The caller must ensure:
    /// - `memory` points to a correctly-sized, correctly-aligned region
    /// - `memory` is zeroed
    /// - `level` is valid for the architecture (`0..NUM_LEVELS`)
    pub fn from_memory(memory: &'a mut [u64], level: usize) -> Result<Self, TableError> {
        if level >= A::NUM_LEVELS {
            return Err(TableError::InvalidLevel);
        }
        let expected = A::entries_per_table(level) * A::ENTRY_WIDTH;
        if memory.len() != expected {
            return Err(TableError::InvalidTableSize);
        }
        Ok(Self {
            entries: memory,
            level,
            _arch: PhantomData,
        })
    }

    /// Wrap an existing populated table.
    ///
    /// Same requirements as `from_memory` except the memory need not be zeroed.
    pub fn from_existing(memory: &'a mut [u64], level: usize) -> Result<Self, TableError> {
        Self::from_memory(memory, level)
    }

    /// The table level (0 = root, NUM_LEVELS-1 = leaf).
    pub fn level(&self) -> usize {
        self.level
    }

    /// Number of entries in this table.
    pub fn num_entries(&self) -> usize {
        self.entries.len() / A::ENTRY_WIDTH
    }

    /// What this level supports.
    pub fn capabilities(&self) -> LevelCapabilities {
        A::level_capabilities(self.level)
    }

    /// Read the raw u64 value at the given index.
    /// For architectures with `ENTRY_WIDTH > 1`, returns only the first word.
    /// Use `read_raw_wide` for the full entry.
    pub fn read_raw(&self, index: usize) -> Result<u64, TableError> {
        let offset = index * A::ENTRY_WIDTH;
        self.entries
            .get(offset)
            .copied()
            .ok_or(TableError::IndexOutOfBounds)
    }

    /// Read the raw u64 words for the entry at the given index.
    /// Returns a slice of `ENTRY_WIDTH` words.
    pub fn read_raw_wide(&self, index: usize) -> Result<&[u64], TableError> {
        let offset = index * A::ENTRY_WIDTH;
        self.entries
            .get(offset..offset + A::ENTRY_WIDTH)
            .ok_or(TableError::IndexOutOfBounds)
    }

    /// Read and decode the entry at the given index.
    pub fn read_entry(&self, index: usize) -> Result<EntryKind, TableError> {
        let raw = self.read_raw_wide(index)?;
        Ok(A::decode_entry_wide(raw, self.level))
    }

    /// Write a table pointer entry at the given index.
    ///
    /// The entry must currently be invalid (clear it first if overwriting).
    pub fn set_table_entry(
        &mut self,
        index: usize,
        next_table_phys: PhysAddr,
    ) -> Result<(), TableError> {
        if !A::level_capabilities(self.level).supports_table_pointer {
            return Err(TableError::TablePointerNotSupported);
        }
        let offset = index * A::ENTRY_WIDTH;
        let slot = self
            .entries
            .get(offset..offset + A::ENTRY_WIDTH)
            .ok_or(TableError::IndexOutOfBounds)?;
        if A::decode_entry_wide(slot, self.level) != EntryKind::Invalid {
            return Err(TableError::EntryAlreadyValid);
        }
        let encoded = A::encode_table_entry(next_table_phys, self.level);
        self.entries[offset] = encoded;
        Ok(())
    }

    /// Write a block (or page at leaf level) mapping entry.
    ///
    /// The entry must currently be invalid.
    pub fn set_block_entry(
        &mut self,
        index: usize,
        phys: PhysAddr,
        attr: AttributeFields,
    ) -> Result<(), TableError> {
        let caps = A::level_capabilities(self.level);
        if !caps.supports_block {
            return Err(TableError::BlockNotSupported);
        }
        let offset = index * A::ENTRY_WIDTH;
        let slot = self
            .entries
            .get(offset..offset + A::ENTRY_WIDTH)
            .ok_or(TableError::IndexOutOfBounds)?;
        if A::decode_entry_wide(slot, self.level) != EntryKind::Invalid {
            return Err(TableError::EntryAlreadyValid);
        }
        // Use page encoding at the leaf level, block encoding otherwise.
        if self.level == A::NUM_LEVELS - 1 || A::ENTRY_WIDTH > 1 {
            let buf = self
                .entries
                .get_mut(offset..offset + A::ENTRY_WIDTH)
                .ok_or(TableError::IndexOutOfBounds)?;
            A::encode_page_entry_wide(phys, attr, buf);
        } else {
            self.entries[offset] = A::encode_block_entry(phys, attr, self.level);
        };
        Ok(())
    }

    /// Clear (invalidate) the entry at the given index.
    pub fn clear_entry(&mut self, index: usize) -> Result<(), TableError> {
        let offset = index * A::ENTRY_WIDTH;
        let slot = self
            .entries
            .get_mut(offset..offset + A::ENTRY_WIDTH)
            .ok_or(TableError::IndexOutOfBounds)?;
        slot.fill(0);
        Ok(())
    }

    /// Write a raw u64 value at the given index.
    ///
    /// This bypasses all validation — use only when you know exactly
    /// what descriptor bits to set. For architectures with `ENTRY_WIDTH > 1`,
    /// this only writes the first word; use `write_raw_wide` for all words.
    ///
    /// # Safety
    /// The caller must ensure the raw value is a valid descriptor for this level.
    pub unsafe fn write_raw(&mut self, index: usize, value: u64) -> Result<(), TableError> {
        let offset = index * A::ENTRY_WIDTH;
        let slot = self
            .entries
            .get_mut(offset)
            .ok_or(TableError::IndexOutOfBounds)?;
        *slot = value;
        Ok(())
    }

    /// Write raw u64 words at the given index.
    ///
    /// # Safety
    /// The caller must ensure the raw values form a valid descriptor for this level.
    pub unsafe fn write_raw_wide(
        &mut self,
        index: usize,
        values: &[u64],
    ) -> Result<(), TableError> {
        debug_assert_eq!(values.len(), A::ENTRY_WIDTH);
        let offset = index * A::ENTRY_WIDTH;
        let slot = self
            .entries
            .get_mut(offset..offset + A::ENTRY_WIDTH)
            .ok_or(TableError::IndexOutOfBounds)?;
        slot.copy_from_slice(values);
        Ok(())
    }

    /// Iterate over all entries, yielding (index, decoded entry) pairs.
    pub fn iter(&self) -> impl Iterator<Item = (usize, EntryKind)> + '_ {
        let level = self.level;
        self.entries
            .chunks_exact(A::ENTRY_WIDTH)
            .enumerate()
            .map(move |(i, chunk)| (i, A::decode_entry_wide(chunk, level)))
    }

    /// Iterate over only valid (non-invalid) entries.
    pub fn iter_valid(&self) -> impl Iterator<Item = (usize, EntryKind)> + '_ {
        self.iter().filter(|(_, kind)| *kind != EntryKind::Invalid)
    }

    /// Get an immutable view of this table.
    pub fn as_ref(&self) -> TableRef<'_, A> {
        TableRef {
            entries: self.entries,
            level: self.level,
            _arch: PhantomData,
        }
    }
}

impl<'a, A: TranslationArch> TableRef<'a, A> {
    /// Create a read-only view from a slice.
    pub fn from_slice(entries: &'a [u64], level: usize) -> Result<Self, TableError> {
        if level >= A::NUM_LEVELS {
            return Err(TableError::InvalidLevel);
        }
        let expected = A::entries_per_table(level) * A::ENTRY_WIDTH;
        if entries.len() != expected {
            return Err(TableError::InvalidTableSize);
        }
        Ok(Self {
            entries,
            level,
            _arch: PhantomData,
        })
    }

    pub fn level(&self) -> usize {
        self.level
    }

    pub fn num_entries(&self) -> usize {
        self.entries.len() / A::ENTRY_WIDTH
    }

    pub fn read_raw(&self, index: usize) -> Result<u64, TableError> {
        let offset = index * A::ENTRY_WIDTH;
        self.entries
            .get(offset)
            .copied()
            .ok_or(TableError::IndexOutOfBounds)
    }

    pub fn read_raw_wide(&self, index: usize) -> Result<&[u64], TableError> {
        let offset = index * A::ENTRY_WIDTH;
        self.entries
            .get(offset..offset + A::ENTRY_WIDTH)
            .ok_or(TableError::IndexOutOfBounds)
    }

    pub fn read_entry(&self, index: usize) -> Result<EntryKind, TableError> {
        let raw = self.read_raw_wide(index)?;
        Ok(A::decode_entry_wide(raw, self.level))
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, EntryKind)> + '_ {
        let level = self.level;
        self.entries
            .chunks_exact(A::ENTRY_WIDTH)
            .enumerate()
            .map(move |(i, chunk)| (i, A::decode_entry_wide(chunk, level)))
    }

    pub fn iter_valid(&self) -> impl Iterator<Item = (usize, EntryKind)> + '_ {
        self.iter().filter(|(_, kind)| *kind != EntryKind::Invalid)
    }
}
