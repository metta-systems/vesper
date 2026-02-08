use snafu::Snafu;

/// Errors from translation table operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Snafu)]
pub enum TableError {
    /// Entry index is out of bounds for this table level.
    #[snafu(display("entry index out of bounds"))]
    IndexOutOfBounds,
    /// The provided memory slice has wrong size for the given table level.
    #[snafu(display("memory slice has wrong size for table level"))]
    InvalidTableSize,
    /// The table level is not valid for this architecture.
    #[snafu(display("invalid table level for this architecture"))]
    InvalidLevel,
    /// This table level does not support block mappings.
    #[snafu(display("block mappings not supported at this level"))]
    BlockNotSupported,
    /// This table level does not support table pointers.
    #[snafu(display("table pointers not supported at this level"))]
    TablePointerNotSupported,
    /// The provided physical address is not properly aligned for this entry type.
    #[snafu(display("physical address not properly aligned"))]
    MisalignedAddress,
    /// Attempted to overwrite a valid entry without clearing it first.
    #[snafu(display("entry is already valid"))]
    EntryAlreadyValid,
}
