//! Host tests of the chainboot protocol: the rate request's bytes (literal, so
//! the encoding cannot drift), telling a request from a size, the answers, the
//! divisors and the rate check, and when chainboot and the host give up on a new
//! rate; the handoff to the kernel and the console negotiation.

#[cfg(test)]
mod console;
#[cfg(test)]
mod rate;
#[cfg(test)]
mod request;
#[cfg(test)]
mod wait;
