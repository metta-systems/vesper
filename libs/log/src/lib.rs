// Copyright 2014-2015 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

#![no_std]
#![no_main]
#![feature(format_args_nl)]
#![feature(custom_test_frameworks)]
// #![test_runner(libtest::test_runner)]
#![reexport_test_harness_main = "test_main"]

use core::{
    error::Error,
    fmt,
    sync::atomic::{AtomicUsize, Ordering},
};

#[macro_use]
mod macros;
// WARNING: this is not part of the crate's public API and is subject to change at any time
#[doc(hidden)]
pub mod __private_api;

// The LOGGER static holds a pointer to the global logger. It is protected by
// the STATE static which determines whether LOGGER has been initialized yet.
static mut LOGGER: &dyn Log = &NopLogger;

static STATE: AtomicUsize = AtomicUsize::new(0);

// There are three different states that we care about: the logger's
// uninitialized, the logger's initializing (set_logger's been called but
// LOGGER hasn't actually been set yet), or the logger's active.
const UNINITIALIZED: usize = 0;
const INITIALIZING: usize = 1;
const INITIALIZED: usize = 2;

static MAX_LOG_LEVEL_FILTER: AtomicUsize = AtomicUsize::new(0);

static LOG_LEVEL_NAMES: [&str; 6] = ["OFF", "ERROR", "WARN", "INFO", "DEBUG", "TRACE"];

static SET_LOGGER_ERROR: &str = "attempted to set a logger after the logging system \
                                 was already initialized";
static LEVEL_PARSE_ERROR: &str =
    "attempted to convert a string that doesn't match an existing log level";

/// An enum representing the available verbosity levels of the logger.
///
#[repr(isize)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub enum Level {
    Print = -2,
    PrintLn = -1,
    Off = 0,
    Error = 1,
    Warn,
    Info,
    Debug,
    Trace,
}

impl Level {
    fn from_usize(u: usize) -> Option<Level> {
        match u {
            1 => Some(Level::Error),
            2 => Some(Level::Warn),
            3 => Some(Level::Info),
            4 => Some(Level::Debug),
            5 => Some(Level::Trace),
            _ => None,
        }
    }

    /// Returns the most verbose logging level.
    #[inline]
    pub fn max_level() -> Level {
        Level::Trace
    }

    /// Returns the string representation of the `Level`.
    ///
    /// This returns the same string as the `fmt::Display` implementation.
    pub fn as_str(&self) -> &'static str {
        LOG_LEVEL_NAMES.get(*self as usize).map_or("", |v| v)
    }

    /// Iterate through all supported logging levels.
    ///
    /// The order of iteration is from more severe to less severe log messages.
    ///
    /// # Examples
    ///
    /// ```
    /// use log::Level;
    ///
    /// let mut levels = Level::iter();
    ///
    /// assert_eq!(Some(Level::Error), levels.next());
    /// assert_eq!(Some(Level::Trace), levels.last());
    /// ```
    pub fn iter() -> impl Iterator<Item = Self> {
        (1..6).map(|i| Self::from_usize(i).unwrap())
    }

    /// Get the next-highest `Level` from this one.
    ///
    /// If the current `Level` is at the highest level, the returned `Level` will be the same as the
    /// current one.
    ///
    /// # Examples
    ///
    /// ```
    /// use log::Level;
    ///
    /// let level = Level::Info;
    ///
    /// assert_eq!(Level::Debug, level.increment_severity());
    /// assert_eq!(Level::Trace, level.increment_severity().increment_severity());
    /// assert_eq!(Level::Trace, level.increment_severity().increment_severity().increment_severity()); // max level
    /// ```
    #[must_use]
    pub fn increment_severity(&self) -> Self {
        let next = (*self as usize).saturating_add(1);
        Self::from_usize(next).unwrap_or(*self)
    }

    /// Get the next-lowest `Level` from this one.
    ///
    /// If the current `Level` is at the lowest level, the returned `Level` will be the same as the
    /// current one.
    ///
    /// # Examples
    ///
    /// ```
    /// use log::Level;
    ///
    /// let level = Level::Info;
    ///
    /// assert_eq!(Level::Warn, level.decrement_severity());
    /// assert_eq!(Level::Error, level.decrement_severity().decrement_severity());
    /// assert_eq!(Level::Error, level.decrement_severity().decrement_severity().decrement_severity()); // min level
    /// ```
    #[must_use]
    pub fn decrement_severity(&self) -> Self {
        let next = (*self as usize).saturating_sub(1);
        Self::from_usize(next).unwrap_or(*self)
    }
}

#[derive(Clone, Debug)]
pub struct Record<'a> {
    level: Level,
    args: fmt::Arguments<'a>,
}

impl<'a> Record<'a> {
    /// Returns a new builder.
    #[inline]
    pub fn builder() -> RecordBuilder<'a> {
        RecordBuilder::new()
    }

    /// The message body.
    #[inline]
    pub fn args(&self) -> &fmt::Arguments<'a> {
        &self.args
    }

    /// The verbosity level of the message.
    #[inline]
    pub fn level(&self) -> Level {
        self.level
    }
}

#[derive(Debug)]
pub struct RecordBuilder<'a> {
    record: Record<'a>,
}

impl<'a> RecordBuilder<'a> {
    /// Construct new `RecordBuilder`.
    ///
    /// The default options are:
    ///
    /// - `args`: [`format_args!("")`]
    /// - `metadata`: [`Metadata::builder().build()`]
    ///
    /// [`format_args!("")`]: https://doc.rust-lang.org/std/macro.format_args.html
    /// [`Metadata::builder().build()`]: struct.MetadataBuilder.html#method.build
    #[inline]
    pub fn new() -> RecordBuilder<'a> {
        RecordBuilder {
            record: Record {
                args: format_args!(""),
                level: Level::Info,
            },
        }
    }

    /// Set [`args`](struct.Record.html#method.args).
    #[inline]
    pub fn args(&mut self, args: fmt::Arguments<'a>) -> &mut RecordBuilder<'a> {
        self.record.args = args;
        self
    }

    /// Set [`Metadata::level`](struct.Metadata.html#method.level).
    #[inline]
    pub fn level(&mut self, level: Level) -> &mut RecordBuilder<'a> {
        self.record.level = level;
        self
    }

    /// Invoke the builder and return a `Record`
    #[inline]
    pub fn build(&self) -> Record<'a> {
        self.record.clone()
    }
}

impl Default for RecordBuilder<'_> {
    fn default() -> Self {
        Self::new()
    }
}

/// A trait encapsulating the operations required of a logger.
pub trait Log: Sync + Send {
    /// Determines if a log message with the specified metadata would be
    /// logged.
    ///
    /// This is used by the `log_enabled!` macro to allow callers to avoid
    /// expensive computation of log message arguments if the message would be
    /// discarded anyway.
    ///
    /// # For implementors
    ///
    /// This method isn't called automatically by the `log!` macros.
    /// It's up to an implementation of the `Log` trait to call `enabled` in its own
    /// `log` method implementation to guarantee that filtering is applied.
    fn enabled(&self, level: Level) -> bool;

    /// Logs the `Record`.
    ///
    /// # For implementors
    ///
    /// Note that `enabled` is *not* necessarily called before this method.
    /// Implementations of `log` should perform all necessary filtering
    /// internally.
    fn log(&self, record: &Record);

    /// Flushes any buffered records.
    ///
    /// # For implementors
    ///
    /// This method isn't called automatically by the `log!` macros.
    /// It can be called manually on shut-down to ensure any in-flight records are flushed.
    fn flush(&self);
}

/// A dummy initial value for LOGGER.
struct NopLogger;

impl Log for NopLogger {
    fn enabled(&self, _: Level) -> bool {
        false
    }
    fn log(&self, _: &Record) {}
    fn flush(&self) {}
}

impl<T> Log for &'_ T
where
    T: ?Sized + Log,
{
    fn enabled(&self, level: Level) -> bool {
        (**self).enabled(level)
    }

    fn log(&self, record: &Record) {
        (**self).log(record);
    }
    fn flush(&self) {
        (**self).flush();
    }
}

/// Sets the global maximum log level.
///
/// Generally, this should only be called by the active logging implementation.
///
/// Note that `Trace` is the maximum level, because it provides the maximum amount of detail in the emitted logs.
#[inline]
pub fn set_max_level(level: Level) {
    MAX_LOG_LEVEL_FILTER.store(level as usize, Ordering::Relaxed);
}

/// A thread-unsafe version of [`set_max_level`].
///
/// This function is available on all platforms, even those that do not have
/// support for atomics that is needed by [`set_max_level`].
///
/// In almost all cases, [`set_max_level`] should be preferred.
///
/// # Safety
///
/// This function is only safe to call when it cannot race with any other
/// calls to `set_max_level` or `set_max_level_racy`.
///
/// This can be upheld by (for example) making sure that **there are no other
/// threads**, and (on embedded) that **interrupts are disabled**.
///
/// It is safe to use all other logging functions while this function runs
/// (including all logging macros).
///
/// [`set_max_level`]: fn.set_max_level.html
#[inline]
pub unsafe fn set_max_level_racy(level: Level) {
    // `MAX_LOG_LEVEL_FILTER` uses a `Cell` as the underlying primitive when a
    // platform doesn't support `target_has_atomic = "ptr"`, so even though this looks the same
    // as `set_max_level` it may have different safety properties.
    MAX_LOG_LEVEL_FILTER.store(level as usize, Ordering::Relaxed);
}

/// Returns the current maximum log level.
///
/// The [`log!`], [`error!`], [`warn!`], [`info!`], [`debug!`], and [`trace!`] macros check
/// this value and discard any message logged at a higher level. The maximum
/// log level is set by the [`set_max_level`] function.
///
/// [`log!`]: macro.log.html
/// [`error!`]: macro.error.html
/// [`warn!`]: macro.warn.html
/// [`info!`]: macro.info.html
/// [`debug!`]: macro.debug.html
/// [`trace!`]: macro.trace.html
/// [`set_max_level`]: fn.set_max_level.html
#[inline(always)]
pub fn max_level() -> Level {
    // Since `LevelFilter` is `repr(usize)`,
    // this transmute is sound if and only if `MAX_LOG_LEVEL_FILTER`
    // is set to a usize that is a valid discriminant for `LevelFilter`.
    // Since `MAX_LOG_LEVEL_FILTER` is private, the only time it's set
    // is by `set_max_level` above, i.e. by casting a `LevelFilter` to `usize`.
    // So any usize stored in `MAX_LOG_LEVEL_FILTER` is a valid discriminant.
    // SAFETY: We stored a value from the `Level` enum.
    unsafe { core::mem::transmute(MAX_LOG_LEVEL_FILTER.load(Ordering::Relaxed)) }
}

/// Sets the global logger to a `&'static Log`.
///
/// This function may only be called once in the lifetime of a program. Any log
/// events that occur before the call to `set_logger` completes will be ignored.
///
/// This function does not typically need to be called manually. Logger
/// implementations should provide an initialization method that installs the
/// logger internally.
///
/// # Availability
///
/// This method is available even when the `std` feature is disabled. However,
/// it is currently unavailable on `thumbv6` targets, which lack support for
/// some atomic operations which are used by this function. Even on those
/// targets, [`set_logger_racy`] will be available.
///
/// # Errors
///
/// An error is returned if a logger has already been set.
///
/// # Examples
///
/// ```
/// use log::{error, info, warn, Record, Level, Metadata, LevelFilter};
///
/// static MY_LOGGER: MyLogger = MyLogger;
///
/// struct MyLogger;
///
/// impl log::Log for MyLogger {
///     fn enabled(&self, metadata: &Metadata) -> bool {
///         metadata.level() <= Level::Info
///     }
///
///     fn log(&self, record: &Record) {
///         if self.enabled(record.metadata()) {
///             println!("{} - {}", record.level(), record.args());
///         }
///     }
///     fn flush(&self) {}
/// }
///
/// # fn main(){
/// log::set_logger(&MY_LOGGER).unwrap();
/// log::set_max_level(LevelFilter::Info);
///
/// info!("hello log");
/// warn!("warning");
/// error!("oops");
/// # }
/// ```
///
/// [`set_logger_racy`]: fn.set_logger_racy.html
#[cfg(target_has_atomic = "ptr")]
pub fn set_logger(logger: &'static dyn Log) -> Result<(), SetLoggerError> {
    set_logger_inner(|| logger)
}

#[cfg(target_has_atomic = "ptr")]
fn set_logger_inner<F>(make_logger: F) -> Result<(), SetLoggerError>
where
    F: FnOnce() -> &'static dyn Log,
{
    match STATE.compare_exchange(
        UNINITIALIZED,
        INITIALIZING,
        Ordering::Acquire,
        Ordering::Relaxed,
    ) {
        Ok(UNINITIALIZED) => {
            // SAFETY: We're single threaded and protected by an atomic, initialize away.
            unsafe {
                LOGGER = make_logger();
            }
            STATE.store(INITIALIZED, Ordering::Release);
            Ok(())
        }
        Err(INITIALIZING) => {
            while STATE.load(Ordering::Relaxed) == INITIALIZING {
                core::hint::spin_loop();
            }
            Err(SetLoggerError)
        }
        _ => Err(SetLoggerError),
    }
}

/// A thread-unsafe version of [`set_logger`].
///
/// This function is available on all platforms, even those that do not have
/// support for atomics that is needed by [`set_logger`].
///
/// In almost all cases, [`set_logger`] should be preferred.
///
/// # Safety
///
/// This function is only safe to call when it cannot race with any other
/// calls to `set_logger` or `set_logger_racy`.
///
/// This can be upheld by (for example) making sure that **there are no other
/// threads**, and (on embedded) that **interrupts are disabled**.
///
/// It is safe to use other logging functions while this function runs
/// (including all logging macros).
///
/// [`set_logger`]: fn.set_logger.html
pub unsafe fn set_logger_racy(logger: &'static dyn Log) -> Result<(), SetLoggerError> {
    match STATE.load(Ordering::Acquire) {
        UNINITIALIZED => {
            // SAFETY: It's a racy method, we could race!
            unsafe {
                LOGGER = logger;
            }
            STATE.store(INITIALIZED, Ordering::Release);
            Ok(())
        }
        INITIALIZING => {
            // This is just plain UB, since we were racing another initialization function
            unreachable!("set_logger_racy must not be used with other initialization functions")
        }
        _ => Err(SetLoggerError),
    }
}

/// The type returned by [`set_logger`] if [`set_logger`] has already been called.
///
/// [`set_logger`]: fn.set_logger.html
#[allow(missing_copy_implementations)]
#[derive(Debug)]
pub struct SetLoggerError;

impl fmt::Display for SetLoggerError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(SET_LOGGER_ERROR)
    }
}

impl Error for SetLoggerError {}

/// The type returned by [`from_str`] when the string doesn't match any of the log levels.
///
/// [`from_str`]: https://doc.rust-lang.org/std/str/trait.FromStr.html#tymethod.from_str
#[allow(missing_copy_implementations)]
#[derive(Debug, PartialEq, Eq)]
pub struct ParseLevelError;

impl fmt::Display for ParseLevelError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(LEVEL_PARSE_ERROR)
    }
}

impl Error for ParseLevelError {}

/// Returns a reference to the logger.
///
/// If a logger has not been set, a no-op implementation is returned.
pub fn logger() -> &'static dyn Log {
    // Acquire memory ordering guarantees that current thread would see any
    // memory writes that happened before store of the value
    // into `STATE` with memory ordering `Release` or stronger.
    //
    // Since the value `INITIALIZED` is written only after `LOGGER` was
    // initialized, observing it after `Acquire` load here makes both
    // write to the `LOGGER` static and initialization of the logger
    // internal state synchronized with current thread.
    if STATE.load(Ordering::Acquire) == INITIALIZED {
        // SAFETY: We're initialized.
        unsafe { LOGGER }
    } else {
        static NOP: NopLogger = NopLogger;
        &NOP
    }
}
