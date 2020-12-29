/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

//! Syscall API for calling kernel functions.
//!
//! Arch-specific kernel ABI decodes syscall invocations and calls API functions to perform actual
//! operations.

// Syscalls (kernel API)
trait API {
    fn send(cap: Cap, msg_info: MessageInfo);
    // Wait for message, when it is received,
    // return object Badge and block caller on `reply`.
    fn recv(src: Cap, reply: Cap) -> Result<(MessageInfo, Option<&Badge>)>;
    fn call(cap: Cap, msg_info: MessageInfo) -> Result<(MessageInfo, Option<&Badge>)>;
    fn reply(msg_info: MessageInfo);
    fn nb_send(dest: Cap, msg_info: MessageInfo);
    // As Recv but invoke `reply` first.
    fn reply_recv(
        src: Cap,
        reply: Cap,
        msg_info: MessageInfo,
    ) -> Result<(MessageInfo, Option<&Badge>)>;
    // As ReplyRecv but invoke `dest` not `reply`.
    fn nb_send_recv(
        dest: Cap,
        msg_info: MessageInfo,
        src: Cap,
        reply: Cap,
    ) -> Result<(MessageInfo, Options<&Badge>)>;
    fn nb_recv(src: Cap) -> Result<(MessageInfo, Option<&Badge>)>;
    // As NBSendRecv, with no reply. Donation is not possible.
    fn nb_send_wait(
        cap: Cap,
        msg_info: MessageInfo,
        src: Cap,
    ) -> Result<(MessageInfo, Option<&Badge>)>;
    // As per Recv, but donation not possible.
    fn wait(src: Cap) -> Result<(MessageInfo, Option<&Badge>)>;
    fn r#yield();
    // Plus some debugging calls...
}

struct Nucleus {}

impl API for Nucleus {
    fn send(cap: _, msg_info: _) {
        unimplemented!()
    }

    fn recv(src: _, reply: _) -> _ {
        unimplemented!()
    }

    fn call(cap: _, msg_info: _) -> _ {
        unimplemented!()
    }

    fn reply(msg_info: _) {
        unimplemented!()
    }

    fn nb_send(dest: _, msg_info: _) {
        unimplemented!()
    }

    fn reply_recv(src: _, reply: _, msg_info: _) -> _ {
        unimplemented!()
    }

    fn nb_send_recv(dest: _, msg_info: _, src: _, reply: _) -> _ {
        unimplemented!()
    }

    fn nb_recv(src: _) -> _ {
        unimplemented!()
    }

    fn nb_send_wait(cap: _, msg_info: _, src: _) -> _ {
        unimplemented!()
    }

    fn wait(src: _) -> _ {
        unimplemented!()
    }

    fn r#yield() {
        unimplemented!()
    }
}
