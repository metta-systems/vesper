/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 */

use {
    super::{CapError, Capability, TryFrom},
    crate::capdef,
    paste::paste,
    register::{register_bitfields, LocalRegisterCopy},
};

//=====================
// Cap definition
//=====================

register_bitfields! {
    u128,
    // https://ts.data61.csiro.au/publications/csiro_full_text/Lyons_MAH_18.pdf
    // Resume objects, modelled after KeyKOS [Bomberger et al.1992], are a new object type
    // that generalise the “reply capabilities” of baseline seL4. These were capabilities
    // to virtual objects created by the kernel on-the-fly in seL4’s RPC-style call() operation,
    // which sends a message to an endpoint and blocks on a reply. The receiver of the message
    // (i.e. the server) receives the reply capability in a magic “reply slot” in its
    // capability space. The server replies by invoking that capability. Resume objects
    // remove the magic by explicitly representing the reply channel (and the SC-donation chain).
    // They also provide more efficient support for stateful servers that handle concurrent client
    // sessions.
    // The introduction of Resume objects requires some changes to the IPC system-call API.
    // The client-style call() operation is unchanged, but server-side equivalent, ReplyRecv
    // (previously ReplyWait) replies to a previous request and then blocks on the next one.
    // It now must provide an explicit Resume capability; on the send phase, that capability
    // identifies the client and returns the SC if appropriate, on the receive phase it is
    // populated with new values. The new API makes stateful server implementation more efficient.
    // In baseline seL4, the server would have to use at least two extra system calls to save the
    // reply cap and later move it back into its magic slot, removing the magic also removes
    // the need for the extra system calls.

    ResumeCap [
        Type OFFSET(0) NUMBITS(6) [
            value = 22
        ]
    ]
}

capdef! { Resume }

//=====================
// Cap implementation
//=====================
