/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

pub(crate) trait NucleusObject {
    fn size_bits() -> usize;
    fn invoke();
}
