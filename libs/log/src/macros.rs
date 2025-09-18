// Copyright 2014-2015 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

/// Macro similar to [std](https://doc.rust-lang.org/src/std/macros.rs.html)
/// but for writing into kernel-specific output (UART or QEMU console).
#[macro_export]
macro_rules! print {
    ($($arg:tt)+) => ({
        $crate::__private_api::log(
            $crate::__log_logger!(__log_global_logger),
            $crate::__private_api::format_args!($($arg)+),
            $crate::Level::Print,
        );
    });
}

/// Macro similar to [std](https://doc.rust-lang.org/src/std/macros.rs.html)
/// but for writing into kernel-specific output (UART or QEMU console).
#[macro_export]
macro_rules! println {
    ($($arg:tt)+) => ({
        $crate::__private_api::log(
            $crate::__log_logger!(__log_global_logger),
            $crate::__private_api::format_args_nl!($($arg)+),
            $crate::Level::PrintLn,
        );
    });
}

#[macro_export]
#[clippy::format_args]
macro_rules! log {
    // log!(logger: my_logger, Level::Info, "a log event")
    (logger: $logger:expr, $lvl:expr, $($arg:tt)+) => ({
        $crate::__log!(
            logger: $crate::__log_logger!($logger),
            $lvl,
            $($arg)+
        )
    });

    // log!(Level::Info, "a log event")
    ($lvl:expr, $($arg:tt)+) => ({
        $crate::__log!(
            logger: $crate::__log_logger!(__log_global_logger),
            $lvl,
            $($arg)+
        )
    });
}

#[doc(hidden)]
#[macro_export]
macro_rules! __log {
    // log!(logger: my_logger, Level::Info, "a {} event", "log");
    (logger: $logger:expr, $lvl:expr, $($arg:tt)+) => ({
        let lvl = $lvl;
        if lvl <= $crate::max_level() {
            $crate::__private_api::log(
                $logger,
                $crate::__private_api::format_args!($($arg)+),
                lvl,
            );
        }
    });
}

#[macro_export]
#[clippy::format_args]
macro_rules! error {
    // error!(logger: my_logger, key1 = 42, key2 = true; "a {} event", "log")
    // error!(logger: my_logger, "a {} event", "log")
    (logger: $logger:expr, $($arg:tt)+) => ({
        $crate::log!(logger: $crate::__log_logger!($logger), $crate::Level::Error, $($arg)+)
    });

    // error!("a {} event", "log")
    ($($arg:tt)+) => ($crate::log!($crate::Level::Error, $($arg)+))
}

#[macro_export]
#[clippy::format_args]
macro_rules! warn {
    // warn!(logger: my_logger, key1 = 42, key2 = true; "a {} event", "log")
    // warn!(logger: my_logger, "a {} event", "log")
    (logger: $logger:expr, $($arg:tt)+) => ({
        $crate::log!(logger: $crate::__log_logger!($logger), $crate::Level::Warn, $($arg)+)
    });

    // warn!("a {} event", "log")
    ($($arg:tt)+) => ($crate::log!($crate::Level::Warn, $($arg)+))
}

#[macro_export]
#[clippy::format_args]
macro_rules! info {
    // info!(logger: my_logger, key1 = 42, key2 = true; "a {} event", "log")
    // info!(logger: my_logger, "a {} event", "log")
    (logger: $logger:expr, $($arg:tt)+) => ({
        $crate::log!(logger: $crate::__log_logger!($logger), $crate::Level::Info, $($arg)+)
    });

    // info!("a {} event", "log")
    ($($arg:tt)+) => ($crate::log!($crate::Level::Info, $($arg)+))
}

#[macro_export]
#[clippy::format_args]
macro_rules! debug {
    // debug!(logger: my_logger, key1 = 42, key2 = true; "a {} event", "log")
    // debug!(logger: my_logger, "a {} event", "log")
    (logger: $logger:expr, $($arg:tt)+) => ({
        $crate::log!(logger: $crate::__log_logger!($logger), $crate::Level::Debug, $($arg)+)
    });

    // debug!("a {} event", "log")
    ($($arg:tt)+) => ($crate::log!($crate::Level::Debug, $($arg)+))
}

#[macro_export]
#[clippy::format_args]
macro_rules! trace {
    // trace!(logger: my_logger, key1 = 42, key2 = true; "a {} event", "log")
    // trace!(logger: my_logger, "a {} event", "log")
    (logger: $logger:expr, $($arg:tt)+) => ({
        $crate::log!(logger: $crate::__log_logger!($logger), $crate::Level::Trace, $($arg)+)
    });

    // trace!("a {} event", "log")
    ($($arg:tt)+) => ($crate::log!($crate::Level::Trace, $($arg)+))
}

#[macro_export]
macro_rules! log_enabled {
    // log_enabled!(logger: my_logger, Level::Info)
    (logger: $logger:expr, $lvl:expr) => ({
        $crate::__log_enabled!(logger: $crate::__log_logger!($logger), $lvl)
    });

    // log_enabled!(Level::Info)
    ($lvl:expr) => ({
        $crate::__log_enabled!(logger: $crate::__log_logger!(__log_global_logger), $lvl)
    });
}

#[doc(hidden)]
#[macro_export]
macro_rules! __log_enabled {
    // log_enabled!(logger: my_logger, target: "my_target", Level::Info)
    (logger: $logger:expr, $lvl:expr) => {{
        let lvl = $lvl;
        lvl <= $crate::max_level() && $crate::__private_api::enabled($logger, lvl)
    }};
}

// Determine the logger to use, and whether to take it by-value or by reference

#[doc(hidden)]
#[macro_export]
macro_rules! __log_logger {
    (__log_global_logger) => {{ $crate::__private_api::GlobalLogger }};

    ($logger:expr) => {{ &($logger) }};
}
