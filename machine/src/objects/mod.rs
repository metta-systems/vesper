/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 */

// The basic services Vesper provides are as follows:
//
// * _Threads_ are an abstraction of CPU execution that supports running software;
// * _Address spaces_ are virtual memory spaces that each contain an application.
//                    Applications are limited to accessing memory in their address space;
// * _Inter-process communication (IPC)_ via endpoints allows threads to communicate using
//                                       message passing;
// * _Events_ provide a non-blocking signalling mechanism similar to counting semaphores;
// * _Device primitives_ allow device drivers to be implemented as unprivileged applications.
//                       The kernel exports hardware device interrupts via IPC messages; and
// * _Capability spaces_ store capabilities (i.e., access rights) to kernel services along with
//                       their book-keeping information.

pub mod kernel_object;
pub mod untyped;

pub use kernel_object::KernelObject;
