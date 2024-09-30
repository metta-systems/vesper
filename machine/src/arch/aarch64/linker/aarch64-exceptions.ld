// SPDX-FileCopyrightText: 2024 Metta Systems OÜ
// SPDX-FileContributor: Berkus

PROVIDE(current_el0_synchronous   = current_el0_synchronous);
PROVIDE(current_el0_irq           = current_el0_irq);
PROVIDE(current_el0_fiq           = default_exception_handler);
PROVIDE(current_el0_serror        = current_el0_serror);

PROVIDE(current_elx_synchronous   = current_elx_synchronous);
PROVIDE(current_elx_irq           = current_elx_irq);
PROVIDE(current_elx_fiq           = default_exception_handler);
PROVIDE(current_elx_serror        = current_elx_serror);

PROVIDE(lower_aarch64_synchronous = lower_aarch64_synchronous);
PROVIDE(lower_aarch64_irq         = lower_aarch64_irq);
PROVIDE(lower_aarch64_fiq         = default_exception_handler);
PROVIDE(lower_aarch64_serror      = lower_aarch64_serror);

PROVIDE(lower_aarch32_synchronous = lower_aarch32_synchronous);
PROVIDE(lower_aarch32_irq         = lower_aarch32_irq);
PROVIDE(lower_aarch32_fiq         = default_exception_handler);
PROVIDE(lower_aarch32_serror      = lower_aarch32_serror);
