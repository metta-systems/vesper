PROVIDE(current_el0_synchronous   = default_exception_handler);
PROVIDE(current_el0_irq           = default_exception_handler);
PROVIDE(current_el0_fiq           = default_exception_handler);
PROVIDE(current_el0_serror        = default_exception_handler);

PROVIDE(current_elx_synchronous   = default_exception_handler);
PROVIDE(current_elx_irq           = default_exception_handler);
PROVIDE(current_elx_fiq           = default_exception_handler);
PROVIDE(current_elx_serror        = default_exception_handler);

PROVIDE(lower_aarch64_synchronous = default_exception_handler);
PROVIDE(lower_aarch64_irq         = default_exception_handler);
PROVIDE(lower_aarch64_fiq         = default_exception_handler);
PROVIDE(lower_aarch64_serror      = default_exception_handler);

PROVIDE(lower_aarch32_synchronous = default_exception_handler);
PROVIDE(lower_aarch32_irq         = default_exception_handler);
PROVIDE(lower_aarch32_fiq         = default_exception_handler);
PROVIDE(lower_aarch32_serror      = default_exception_handler);
