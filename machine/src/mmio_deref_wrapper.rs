use core::{marker::PhantomData, ops};

//--------------------------------------------------------------------------------------------------
// Public Definitions
//--------------------------------------------------------------------------------------------------

pub struct MMIODerefWrapper<T> {
    pub base_addr: usize, // @fixme why pub??
    phantom: PhantomData<fn() -> T>,
}

//--------------------------------------------------------------------------------------------------
// Public Code
//--------------------------------------------------------------------------------------------------

impl<T> MMIODerefWrapper<T> {
    /// Create an instance.
    ///
    /// # Safety
    ///
    /// You could specify any base address here, no checks.
    pub const unsafe fn new(start_addr: usize) -> Self {
        Self {
            base_addr: start_addr,
            phantom: PhantomData,
        }
    }
}

/// Deref to RegisterBlock
///
/// Allows writing
/// ```
/// self.GPPUD.read()
/// ```
/// instead of something along the lines of
/// ```
/// unsafe { (*GPIO::ptr()).GPPUD.read() }
/// ```
impl<T> ops::Deref for MMIODerefWrapper<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*(self.base_addr as *const _) }
    }
}
