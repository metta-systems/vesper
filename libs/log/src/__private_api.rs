// Copyright 2014-2015 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

pub use core::{format_args, format_args_nl, module_path, stringify};
use {
    crate::{Level, Log, Record, logger},
    core::fmt::Arguments,
};

// Log implementation.

/// The global logger proxy.
#[derive(Debug)]
pub struct GlobalLogger;

impl Log for GlobalLogger {
    fn enabled(&self, level: Level) -> bool {
        logger().enabled(level)
    }

    fn log(&self, record: &Record) {
        logger().log(record);
    }

    fn flush(&self) {
        logger().flush();
    }
}

// Split from `log` to reduce generics and code size
fn log_impl<L: Log>(logger: L, args: Arguments, level: Level) {
    let mut builder = Record::builder();

    builder.args(args).level(level);

    logger.log(&builder.build());
}

pub fn log<L: Log>(logger: L, args: Arguments, level: Level) {
    log_impl(logger, args, level);
}

pub fn enabled<L: Log>(logger: L, level: Level) -> bool {
    logger.enabled(level)
}
