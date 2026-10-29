//! Progress lines for the people bringing USB up: on the log channel when the
//! image has a log server, else on the serial channel when the component has
//! one, and on the semihosting console under QEMU. Without either the lines
//! wait in the trace backlog for `usb_v1.trace`.

pub use libserial_trace::Trace;

/// `trace!(trace, "text {value}")`: emit one line.
macro_rules! trace {
    ($trace:expr, $($argument:tt)*) => {
        $trace.line(format_args!($($argument)*))
    };
}
