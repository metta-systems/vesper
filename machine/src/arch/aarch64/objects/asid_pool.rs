/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 */

// implemented for x86 and arm
trait ASIDPool {
    fn assign(virt_space: VirtSpace /*Cap*/) -> Result<()>;
}
