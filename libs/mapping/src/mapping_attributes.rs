use core::fmt::{self, Formatter};

/// Architecture agnostic memory attributes.
#[derive(Copy, Clone, Debug, Eq, PartialOrd, PartialEq)]
pub enum MemAttributes {
    /// Regular memory
    CacheableDRAM,
    /// Memory without caching
    NonCacheableDRAM,
    /// Device memory
    Device,
}

/// Architecture agnostic memory region access permissions.
#[derive(Copy, Clone, Debug, Eq, PartialOrd, PartialEq)]
pub enum AccessPermissions {
    /// Read-write access
    ReadWrite,
    /// Read-only access
    ReadOnly,
}

/// Summary structure of memory region properties.
#[derive(Copy, Clone, Debug, Eq, PartialOrd, PartialEq)]
pub struct AttributeFields {
    /// Attributes
    pub mem_attributes: MemAttributes,
    /// Permissions
    pub acc_perms: AccessPermissions,
    /// Disable executable code in this region
    pub executable: bool,
    /// Is the region occupied or free (use occupied for const init)
    pub occupied: bool,
    /// If this memory can be reclaimed into an Untyped after `kickstart` completes
    pub droppable: bool,
}

//--------------------------------------------------------------------------------------------------
// Public Code
//--------------------------------------------------------------------------------------------------

impl AttributeFields {
    pub const fn defaulted() -> AttributeFields {
        AttributeFields {
            mem_attributes: MemAttributes::CacheableDRAM,
            acc_perms: AccessPermissions::ReadWrite,
            executable: false,
            occupied: false,
            droppable: false,
        }
    }
}

impl Default for AttributeFields {
    fn default() -> Self {
        Self::defaulted()
    }
}

/// Human-readable output of `AttributeFields`
impl fmt::Display for AttributeFields {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let attr = match self.mem_attributes {
            MemAttributes::CacheableDRAM => "C",
            MemAttributes::NonCacheableDRAM => "NC",
            MemAttributes::Device => "Dev",
        };

        let acc_p = match self.acc_perms {
            AccessPermissions::ReadOnly => "RO",
            AccessPermissions::ReadWrite => "RW",
        };

        let xn = if self.executable { "PX" } else { "PXN" };

        let marker = if self.droppable {
            "Drop"
        } else {
            if self.occupied { "Used" } else { "Free" }
        };

        write!(f, "({marker}) {attr: <3} {acc_p} {xn: <3}")
    }
}
