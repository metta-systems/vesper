/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 */

trait IRQControl {
    fn get(irq: u32, dest: CapNodeRootedPath) -> Result<()>;
    // ARM specific?
    fn get_trigger();
    fn get_trigger_core();
}
