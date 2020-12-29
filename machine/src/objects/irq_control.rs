/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

trait IRQControl {
    fn get(irq: u32, dest: CapNodeRootedPath) -> Result<()>;
    // ARM specific?
    fn get_trigger();
    fn get_trigger_core();
}
