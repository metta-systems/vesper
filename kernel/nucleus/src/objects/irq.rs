//! The nucleus IRQ table: one record per line of the installed
//! interrupt-controller component.
//!
//! A record tracks whether its line has a live `IRQHandler`, the generation
//! that handler's capabilities carry, and the binding `SetNotification`
//! made: the Notification to signal with its badge and the receiver Thread
//! that waits on it. `Retire` bumps the generation, so every copy of the
//! retired handler's key fails, and frees the line for a fresh `Issue`.
//!
//! Kickstart carves the records from the boot Untyped, sized by the
//! component's line count; the kernel never allocates them.

use {
    crate::objects::{carved::CarvedId, object_pool::IdentityError, thread::ThreadId},
    libobject::CapError,
};

/// Identity of an `IRQHandler`: its line and the line's generation when it
/// was issued.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IrqHandlerId {
    pub line: u32,
    pub generation: u32,
}

/// What a bound line delivers to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IrqBinding {
    /// Signalled on each interrupt
    pub notification: CarvedId,
    /// The bits signalled: the Notification capability's nonzero badge
    pub badge: u64,
    /// The Thread waiting on the Notification; it owns a Time
    pub receiver: ThreadId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LineState {
    /// No live `IRQHandler`
    Free,
    /// One live `IRQHandler` of the record's generation
    Active,
    /// The generation is exhausted: the line is never issued again
    Exhausted,
}

/// One line's record.
#[derive(Clone, Copy, Debug)]
pub struct IrqLine {
    state: LineState,
    generation: u32,
    binding: Option<IrqBinding>,
}

impl IrqLine {
    /// A line with no handler.
    pub const FREE: Self = Self {
        state: LineState::Free,
        generation: 1,
        binding: None,
    };
}

/// The records of every line, in kernel memory carved at boot.
pub struct IrqTable {
    lines: *mut IrqLine,
    line_count: u32,
}

impl IrqTable {
    /// A table with no lines: no component is installed.
    pub const fn empty() -> Self {
        Self {
            lines: core::ptr::null_mut(),
            line_count: 0,
        }
    }

    /// Initialize `line_count` free records at `lines` and take them over.
    ///
    /// # Safety
    /// `lines` must be aligned for [`IrqLine`] and point to `line_count`
    /// records' worth of kernel memory that nothing else uses for as long
    /// as the table exists.
    pub unsafe fn initialize(lines: *mut IrqLine, line_count: u32) -> Self {
        for index in 0..line_count as usize {
            // SAFETY: the caller guarantees `line_count` writable records.
            unsafe {
                lines.add(index).write(IrqLine::FREE);
            }
        }
        Self { lines, line_count }
    }

    /// Number of lines the table covers.
    pub fn line_count(&self) -> u32 {
        self.line_count
    }

    fn records(&self) -> &[IrqLine] {
        if self.lines.is_null() {
            return &[];
        }
        // SAFETY: `initialize` took over `line_count` initialized records.
        unsafe { core::slice::from_raw_parts(self.lines, self.line_count as usize) }
    }

    fn records_mut(&mut self) -> &mut [IrqLine] {
        if self.lines.is_null() {
            return &mut [];
        }
        // SAFETY: as `records`; `&mut self` makes this the only reference.
        unsafe { core::slice::from_raw_parts_mut(self.lines, self.line_count as usize) }
    }

    /// Give `line` a live handler. A line with one, or whose generations are
    /// exhausted, is `IrqLineActive`; a line beyond the table is
    /// `InvalidOperation`.
    pub fn issue(&mut self, line: u32) -> Result<IrqHandlerId, CapError> {
        let record = self
            .records_mut()
            .get_mut(line as usize)
            .ok_or(CapError::InvalidOperation)?;
        if record.state != LineState::Free {
            return Err(CapError::IrqLineActive { line });
        }
        record.state = LineState::Active;
        record.binding = None;
        Ok(IrqHandlerId {
            line,
            generation: record.generation,
        })
    }

    /// Whether `line` can be issued: in the table and without a handler.
    pub fn check_issue(&self, line: u32) -> Result<(), CapError> {
        match self.records().get(line as usize) {
            None => Err(CapError::InvalidOperation),
            Some(record) if record.state == LineState::Free => Ok(()),
            Some(_) => Err(CapError::IrqLineActive { line }),
        }
    }

    /// The live record `id` names.
    fn live(&mut self, id: IrqHandlerId) -> Result<&mut IrqLine, IdentityError> {
        let record = self
            .records_mut()
            .get_mut(id.line as usize)
            .ok_or(IdentityError::Malformed)?;
        if record.state != LineState::Active || record.generation != id.generation {
            return Err(IdentityError::Retired);
        }
        Ok(record)
    }

    /// Check that `id` names a live handler.
    pub fn validate(&mut self, id: IrqHandlerId) -> Result<(), IdentityError> {
        self.live(id).map(|_| ())
    }

    /// Bind `id`'s line, replacing any earlier binding.
    pub fn bind(&mut self, id: IrqHandlerId, binding: IrqBinding) -> Result<(), IdentityError> {
        self.live(id)?.binding = Some(binding);
        Ok(())
    }

    /// Remove `id`'s binding.
    pub fn clear(&mut self, id: IrqHandlerId) -> Result<(), IdentityError> {
        self.live(id)?.binding = None;
        Ok(())
    }

    /// Retire `id`: drop its binding and bump the line's generation, so
    /// every capability naming `id` fails, and free the line. A generation
    /// at its maximum leaves the line exhausted for good.
    pub fn retire(&mut self, id: IrqHandlerId) -> Result<(), IdentityError> {
        let record = self.live(id)?;
        record.binding = None;
        match record.generation.checked_add(1) {
            Some(next) => {
                record.generation = next;
                record.state = LineState::Free;
            }
            None => record.state = LineState::Exhausted,
        }
        Ok(())
    }

    /// Set the generation of the free `line`, so tests reach exhaustion.
    #[cfg(test)]
    pub fn set_free_generation(&mut self, line: u32, generation: u32) {
        let record = &mut self.records_mut()[line as usize];
        assert_eq!(record.state, LineState::Free);
        record.generation = generation;
    }

    /// The binding of `line`, if it has a live, bound handler.
    pub fn binding(&self, line: u32) -> Option<IrqBinding> {
        self.records()
            .get(line as usize)
            .filter(|record| record.state == LineState::Active)
            .and_then(|record| record.binding)
    }
}
