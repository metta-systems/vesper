// use {
//     crate::{
//         console::{interface, null_console::NullConsole},
//         devices::serial::SerialOps,
//         platform::raspberrypi::device_driver::{mini_uart::MiniUart, pl011_uart::PL011Uart},
//         synchronization::IRQSafeNullLock,
//     },
//     core::fmt,
// };
//
// //--------------------------------------------------------------------------------------------------
// // Private Definitions
// //--------------------------------------------------------------------------------------------------
//
// /// The mutex protected part.
// struct ConsoleInner {
//     output: Output,
// }
//
// //--------------------------------------------------------------------------------------------------
// // Public Definitions
// //--------------------------------------------------------------------------------------------------
//
// /// The main struct.
// pub struct Console {
//     inner: IRQSafeNullLock<ConsoleInner>,
// }
//
// //--------------------------------------------------------------------------------------------------
// // Global instances
// //--------------------------------------------------------------------------------------------------
//
// static CONSOLE: Console = Console::new();
//
// //--------------------------------------------------------------------------------------------------
// // Private Code
// //--------------------------------------------------------------------------------------------------
//
// impl ConsoleInner {
//     pub const fn new() -> Self {
//         Self {
//             output: Output::None(NullConsole {}),
//         }
//     }
//
//     fn current_ptr(&self) -> &dyn interface::ConsoleOps {
//         match &self.output {
//             Output::None(inner) => inner,
//             Output::MiniUart(inner) => inner,
//             Output::Uart(inner) => inner,
//         }
//     }
//
//     /// Overwrite the current output. The old output will go out of scope and
//     /// its Drop function will be called.
//     pub fn replace_with(&mut self, new_output: Output) {
//         self.current_ptr().flush(); // crashed here with Data Abort
//                                     // ...with ESR 0x25/0x96000000
//                                     // ...with FAR 0x984f800000028
//                                     // ...with ELR 0x946a8
//
//         self.output = new_output;
//     }
// }
//
// /// Implementing `core::fmt::Write` enables usage of the `format_args!` macros, which in turn are
// /// used to implement the `kernel`'s `print!` and `println!` macros. By implementing `write_str()`,
// /// we get `write_fmt()` automatically.
// /// See src/macros.rs.
// ///
// /// The function takes an `&mut self`, so it must be implemented for the inner struct.
// impl fmt::Write for ConsoleInner {
//     fn write_str(&mut self, s: &str) -> fmt::Result {
//         self.current_ptr().write_string(s);
//         // for c in s.chars() {
//         //     // Convert newline to carrige return + newline.
//         //     if c == '\n' {
//         //         self.write_char('\r')
//         //     }
//         //
//         //     self.write_char(c);
//         // }
//
//         Ok(())
//     }
// }
//
// //--------------------------------------------------------------------------------------------------
// // Public Code
// //--------------------------------------------------------------------------------------------------
//
// impl Console {
//     /// Create a new instance.
//     pub const fn new() -> Console {
//         Console {
//             inner: NullLock::new(ConsoleInner::new()),
//         }
//     }
//
//     pub fn replace_with(&mut self, new_output: Output) {
//         self.inner.lock(|inner| inner.replace_with(new_output));
//     }
// }
//
// /// The global console. Output of the kernel print! and println! macros goes here.
// pub fn console() -> &'static dyn crate::console::interface::All {
//     &CONSOLE
// }
//
// //------------------------------------------------------------------------------
// // OS Interface Code
// //------------------------------------------------------------------------------
// use crate::synchronization::interface::Mutex;
//
// /// Passthrough of `args` to the `core::fmt::Write` implementation, but guarded by a Mutex to
// /// serialize access.
// impl interface::Write for Console {
//     fn write_fmt(&self, args: core::fmt::Arguments) -> fmt::Result {
//         self.inner.lock(|inner| fmt::Write::write_fmt(inner, args))
//     }
// }
//
// /// Dispatch the respective function to the currently stored output device.
// impl interface::ConsoleOps for Console {
//     // @todo implement utf8 serialization here!
//     fn write_char(&self, c: char) {
//         self.inner.lock(|con| con.current_ptr().write_char(c));
//     }
//
//     fn write_string(&self, string: &str) {
//         self.inner
//             .lock(|con| con.current_ptr().write_string(string));
//     }
//
//     // @todo implement utf8 deserialization here!
//     fn read_char(&self) -> char {
//         self.inner.lock(|con| con.current_ptr().read_char())
//     }
// }
//
// impl SerialOps for Console {
//     fn read_byte(&self) -> u8 {
//         self.inner.lock(|con| con.current_ptr().read_byte())
//     }
//     fn write_byte(&self, byte: u8) {
//         self.inner.lock(|con| con.current_ptr().write_byte(byte))
//     }
//     fn flush(&self) {
//         self.inner.lock(|con| con.current_ptr().flush())
//     }
//     fn clear_rx(&self) {
//         self.inner.lock(|con| con.current_ptr().clear_rx())
//     }
// }
//
// impl interface::All for Console {}
//
// impl Default for Console {
//     fn default() -> Self {
//         Self::new()
//     }
// }
//
// impl Drop for Console {
//     fn drop(&mut self) {}
// }
//
// //------------------------------------------------------------------------------
// // Device Interface Code
// //------------------------------------------------------------------------------
//
// /// Possible outputs which the console can store.
// enum Output {
//     None(NullConsole),
//     MiniUart(MiniUart),
//     Uart(PL011Uart),
// }
//
// /// Generate boilerplate for converting into one of Output enum values
// macro make_from($optname:ident, $name:ty) {
//     impl From<$name> for Output {
//         fn from(instance: $name) -> Self {
//             Output::$optname(instance)
//         }
//     }
// }
//
// make_from!(None, NullConsole);
// make_from!(MiniUart, PreparedMiniUart);
// make_from!(Uart, PreparedPL011Uart);
