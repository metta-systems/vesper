use {
    crate::{CapError, Key, KeyTableKey, RawKey, decode_syscall_result},
    core::fmt,
};

#[cfg(not(test))]
use libsyscall::{protected_call0, protected_call2, protected_call6};
#[cfg(test)]
use tests::{protected_call0, protected_call2, protected_call6};

#[cfg(test)]
#[path = "../tests/support/irq.rs"]
mod tests;

// ==================================================
// == Public user interface, usable from userspace ==
// ==================================================

/// Most cells of an interrupt specifier `IRQControl.Issue` accepts.
pub const MAX_ISSUE_CELLS: usize = 3;

/// Userspace handle to the `IRQControl` capability (at
/// `KeySlot::IRQ_CONTROL` in the boot table): the only authority that issues
/// [`IrqHandlerKey`]s for interrupt lines.
pub struct IrqControlKey {
    key: Key<IrqControlType>,
}

enum IrqControlType {}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum IrqControlOp {
    /// Translate an interrupt specifier into a line and install an
    /// `IRQHandler` for it.
    Issue = 0,
}

impl TryFrom<u64> for IrqControlOp {
    type Error = CapError;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Issue),
            _ => Err(CapError::InvalidOperation),
        }
    }
}

/// An `IRQHandler` installed by [`IrqControlKey::issue`].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct IssuedIrq {
    /// The `IRQHandler` key in the destination table.
    pub handler: RawKey,
    /// The line the specifier decoded to.
    pub line: u32,
}

impl IrqControlKey {
    /// Construct a non-owning handle without installing or validating
    /// authority.
    pub const fn from_key(key: RawKey) -> Self {
        Self { key: Key::new(key) }
    }

    /// The wire encoding of the underlying key.
    pub const fn to_wire(&self) -> u64 {
        self.key.to_wire()
    }

    /// `Issue`: decode the device tree interrupt specifier `cells` (1 to
    /// [`MAX_ISSUE_CELLS`] cells), relative to the interrupt controller at
    /// `controller` in the interrupt-controller component's list (the root
    /// controller is 0), configure the line's trigger from it, and install an
    /// `IRQHandler` for the line at `dst_slot` in `dst_table`. The line stays
    /// masked until the first [`IrqHandlerKey::ack`].
    ///
    /// Wire schema: `x2` destination-table key, `x3` destination slot, `x4`
    /// cell count in bits 7:0 and `controller` in bits 15:8, `x5..x7` the
    /// cells (unused ones zero). Returns the handler key and the line.
    /// Authority: `GRANT` on the `IRQControl`, `INSTALL` on the table.
    /// A line with a live `IRQHandler`, or the nucleus timer line, fails with
    /// [`CapError::IrqLineActive`]; an undecodable specifier with
    /// [`CapError::InvalidOperation`].
    pub fn issue(
        &self,
        dst_table: &KeyTableKey,
        dst_slot: u32,
        controller: u8,
        cells: &[u32],
    ) -> Result<IssuedIrq, CapError> {
        if cells.is_empty() || cells.len() > MAX_ISSUE_CELLS {
            return Err(CapError::InvalidOperation);
        }
        let mut words = [0_u64; MAX_ISSUE_CELLS];
        for (word, &cell) in words.iter_mut().zip(cells) {
            *word = u64::from(cell);
        }
        // The length was checked against MAX_ISSUE_CELLS above.
        let count = cells.len() as u64;
        // SAFETY: the syscall transport is the encapsulated unsafe boundary.
        let result = unsafe {
            protected_call6(
                self.key.to_wire(),
                IrqControlOp::Issue as u64,
                dst_table.to_wire(),
                u64::from(dst_slot),
                count | (u64::from(controller) << 8),
                words[0],
                words[1],
                words[2],
            )
        };
        let (handler, line) = decode_syscall_result(result)?;
        Ok(IssuedIrq {
            handler: RawKey::from_wire(handler),
            line: u32::try_from(line).map_err(|_wide_line| CapError::Unknown)?,
        })
    }
}

impl fmt::Debug for IrqControlKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IrqControlKey")
            .field("key", &self.key)
            .finish()
    }
}

/// Userspace handle to an `IRQHandler` capability: one interrupt line.
///
/// A driver binds the line to a Notification and a receiver Thread with
/// [`Self::set_notification`], enables it with [`Self::ack`], and acks again
/// after handling each interrupt: the nucleus masks the line on every
/// delivery.
pub struct IrqHandlerKey {
    key: Key<IrqHandlerType>,
}

enum IrqHandlerType {}

/// What `IRQHandler.Query` reports for a line.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct IrqStats {
    /// Interrupts handled.
    pub handled: u64,
    /// Time spent handling them, in nanoseconds, rounded up.
    pub nanoseconds: u64,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum IrqHandlerOp {
    /// Bind the line to a Notification and a receiver Thread.
    SetNotification = 0,
    /// Remove the binding.
    Clear = 1,
    /// Enable the line.
    Ack = 2,
    /// Tear the handler down: clear, mask and free the line.
    Retire = 3,
    /// Report the interrupts handled on the line and the time they took.
    Query = 4,
}

impl TryFrom<u64> for IrqHandlerOp {
    type Error = CapError;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::SetNotification),
            1 => Ok(Self::Clear),
            2 => Ok(Self::Ack),
            3 => Ok(Self::Retire),
            4 => Ok(Self::Query),
            _ => Err(CapError::InvalidOperation),
        }
    }
}

impl IrqHandlerKey {
    /// Construct a non-owning handle without installing or validating
    /// authority.
    pub const fn from_key(key: RawKey) -> Self {
        Self { key: Key::new(key) }
    }

    /// The wire encoding of the underlying key.
    pub const fn to_wire(&self) -> u64 {
        self.key.to_wire()
    }

    /// `SetNotification`: on each interrupt, signal `notification` with its
    /// badge (which must be nonzero) for the receiver Thread `receiver`,
    /// which must have its own Time. A line that is already bound fails with /// `AlreadyMapped`; [`Self::clear`] removes the binding first.
    ///
    /// Wire schema: `x2` Notification key (`SEND`), `x3` Thread key
    /// (`WRITE`). Authority: `WRITE` on the handler.
    pub fn set_notification(&self, notification: RawKey, receiver: RawKey) -> Result<(), CapError> {
        // SAFETY: the syscall transport is the encapsulated unsafe boundary.
        let result = unsafe {
            protected_call2(
                self.key.to_wire(),
                IrqHandlerOp::SetNotification as u64,
                notification.to_wire(),
                receiver.to_wire(),
            )
        };
        decode_syscall_result(result).map(|_| ())
    }

    /// `Clear`: remove the binding; a later interrupt is masked and dropped.
    /// Authority: `WRITE`.
    pub fn clear(&self) -> Result<(), CapError> {
        self.call0(IrqHandlerOp::Clear)
    }

    /// `Ack`: enable the line. Authority: `WRITE`.
    pub fn ack(&self) -> Result<(), CapError> {
        self.call0(IrqHandlerOp::Ack)
    }

    /// `Query`: the interrupts the nucleus has handled on the line and the
    /// nanoseconds it spent handling them (rounded up), cumulative for the
    /// line. Authority: `READ`.
    pub fn query(&self) -> Result<IrqStats, CapError> {
        // SAFETY: the syscall transport is the encapsulated unsafe boundary.
        let result = unsafe { protected_call0(self.key.to_wire(), IrqHandlerOp::Query as u64) };
        let (handled, nanoseconds) = decode_syscall_result(result)?;
        Ok(IrqStats {
            handled,
            nanoseconds,
        })
    }

    /// `Retire`: clear the binding, mask the line and free it for a fresh
    /// `IRQControl.Issue`; every copy of this key stops working. Authority:
    /// `RETIRE`.
    pub fn retire(&self) -> Result<(), CapError> {
        self.call0(IrqHandlerOp::Retire)
    }

    fn call0(&self, op: IrqHandlerOp) -> Result<(), CapError> {
        // SAFETY: the syscall transport is the encapsulated unsafe boundary.
        let result = unsafe { protected_call0(self.key.to_wire(), op as u64) };
        decode_syscall_result(result).map(|_| ())
    }
}

impl fmt::Debug for IrqHandlerKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IrqHandlerKey")
            .field("key", &self.key)
            .finish()
    }
}
