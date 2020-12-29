/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

// implemented for x86 and arm
trait ASIDPool {
    fn assign(virt_space: VirtSpace /*Cap*/) -> Result<()>;
}
