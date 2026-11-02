//! Host tests of the input event contract: the wire layout of the record and the
//! descriptor (literal bytes, so the encoding cannot drift), usage constants, the
//! decode refusals and the state tracker.

#[cfg(test)]
mod descriptor;
#[cfg(test)]
mod record;
#[cfg(test)]
mod state;
#[cfg(test)]
mod usage;
