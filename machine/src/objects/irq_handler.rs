/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

trait IRQHandler {
    fn set_notification(notification: CapNode) -> Result<()>;
    fn ack() -> Result<()>;
    fn clear() -> Result<()>;
}
