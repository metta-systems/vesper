#![no_std]
#![no_main]

//! irq-test's EL0 interrupt receiver (see `irq-protocol`). It enables its
//! line with `Ack`, then for each interrupt reports the delivered bits,
//! waits for the builder to quiet the device and acks again. After
//! [`ROUNDS`] interrupts it parks on its interrupt Notification for good.

use {
    irq_protocol::{ROUNDS, receiver_init},
    libobject::{IrqHandlerKey, NotificationKey, RawKey},
};

libuser::entry!(main);

fn notification(key: u64) -> NotificationKey {
    NotificationKey::from_key(RawKey::from_wire(key))
}

fn main(_argument: u64) -> ! {
    // SAFETY: the builder fills the init page before this Thread runs.
    let init = unsafe { receiver_init() };
    let (irq, report, resume) = (
        notification(init.irq),
        notification(init.report),
        notification(init.resume),
    );
    let handler = IrqHandlerKey::from_key(RawKey::from_wire(init.handler));

    // A freshly issued line stays masked until its first Ack.
    handler.ack().unwrap_or_else(|_| panic!("first Ack failed"));
    for _ in 0..ROUNDS {
        let bits = irq
            .await_bits(NotificationKey::WAIT_INFINITE)
            .unwrap_or_else(|_| panic!("interrupt Wait failed"));
        report
            .signal(bits)
            .unwrap_or_else(|_| panic!("report failed"));
        resume
            .await_bits(NotificationKey::WAIT_INFINITE)
            .unwrap_or_else(|_| panic!("resume Wait failed"));
        handler.ack().unwrap_or_else(|_| panic!("Ack failed"));
    }
    loop {
        let _bits = irq.await_bits(NotificationKey::WAIT_INFINITE);
    }
}
