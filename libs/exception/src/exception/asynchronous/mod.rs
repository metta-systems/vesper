// #[cfg(target_arch = "aarch64")]
// use crate::arch::aarch64::exception::asynchronous as arch_asynchronous;
use core::marker::PhantomData;

//--------------------------------------------------------------------------------------------------
// Architectural Public Reexports
//--------------------------------------------------------------------------------------------------
pub use liblocal_irq::exception::asynchronous::{
    is_local_irq_masked, local_irq_mask, local_irq_mask_save, local_irq_restore, local_irq_unmask,
    print_state,
};

//--------------------------------------------------------------------------------------------------
// Public Definitions
//--------------------------------------------------------------------------------------------------

// Interrupt number as defined by the BSP.
// pub type IRQNumber = crate::platform::exception::asynchronous::IRQNumber;

/// Interrupt descriptor.
#[derive(Copy, Clone)]
pub struct IRQHandlerDescriptor<T>
where
    T: Copy,
{
    /// The IRQ number.
    number: T,

    /// Descriptive name.
    name: &'static str,

    /// Reference to handler trait object.
    handler: &'static (dyn interface::IRQHandler + Sync),
}

/// IRQContext token.
///
/// An instance of this type indicates that the local core is currently executing in IRQ
/// context, aka executing an interrupt vector or subcalls of it.
///
/// Concept and implementation derived from the `CriticalSection` introduced in
/// <https://github.com/rust-embedded/bare-metal>
#[derive(Clone, Copy)]
pub struct IRQContext<'irq_context> {
    _0: PhantomData<&'irq_context ()>,
}

/// Asynchronous exception handling interfaces.
pub mod interface {
    /// Implemented by types that handle IRQs.
    pub trait IRQHandler {
        /// Called when the corresponding interrupt is asserted.
        fn handle(&self) -> Result<(), &'static str>;
    }

    /// IRQ management functions.
    ///
    /// The `BSP` is supposed to supply one global instance. Typically implemented by the
    /// platform's interrupt controller.
    pub trait IRQManager {
        /// The IRQ number type depends on the implementation.
        type IRQNumberType: Copy;

        /// Register a handler.
        fn register_handler(
            &self,
            irq_handler_descriptor: super::IRQHandlerDescriptor<Self::IRQNumberType>,
        ) -> Result<(), &'static str>;

        /// Enable an interrupt in the controller.
        fn enable(&self, irq_number: &Self::IRQNumberType);

        /// Handle pending interrupts.
        ///
        /// This function is called directly from the CPU's IRQ exception vector. On AArch64,
        /// this means that the respective CPU core has disabled exception handling.
        /// This function can therefore not be preempted and runs start to finish.
        ///
        /// Takes an IRQContext token to ensure it can only be called from IRQ context.
        #[allow(clippy::trivially_copy_pass_by_ref)]
        fn handle_pending_irqs<'irq_context>(
            &'irq_context self,
            ic: &super::IRQContext<'irq_context>,
        );

        /// Print list of registered handlers.
        fn print_handler(&self) {}
    }
}

//--------------------------------------------------------------------------------------------------
// Public Code
//--------------------------------------------------------------------------------------------------

impl<T> IRQHandlerDescriptor<T>
where
    T: Copy,
{
    /// Create an instance.
    pub const fn new(
        number: T,
        name: &'static str,
        handler: &'static (dyn interface::IRQHandler + Sync),
    ) -> Self {
        Self {
            number,
            name,
            handler,
        }
    }

    /// Return the number.
    pub const fn number(&self) -> T {
        self.number
    }

    /// Return the name.
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// Return the handler.
    pub const fn handler(&self) -> &'static (dyn interface::IRQHandler + Sync) {
        self.handler
    }
}

impl IRQContext<'_> {
    /// Creates an IRQContext token.
    ///
    /// # Safety
    ///
    /// - This must only be called when the current core is in an interrupt context and will not
    ///   live beyond the end of it. That is, creation is allowed in interrupt vector functions. For
    ///   example, in the ARMv8-A case, in `extern "C" fn current_elx_irq()`.
    /// - Note that the lifetime `'irq_context` of the returned instance is unconstrained. User code
    ///   must not be able to influence the lifetime picked for this type, since that might cause it
    ///   to be inferred to `'static`.
    #[inline(always)]
    pub unsafe fn new() -> Self {
        IRQContext { _0: PhantomData }
    }
}
