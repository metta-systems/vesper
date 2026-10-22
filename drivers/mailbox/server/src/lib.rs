#![no_std]

//! The `mailbox_v1` interface, generated from
//! `drivers/mailbox/interfaces/mailbox_v1.if`: its types, the
//! [`mailbox_v1::MailboxV1`] trait the mailbox component implements, and
//! [`mailbox_v1_export!`], which exports an implementation's functions as
//! PPC entries.

include!(concat!(env!("OUT_DIR"), "/mailbox_v1.rs"));
