#![no_std]

//! Support code shared by every Meddle-generated interface crate.
//!
//! `meddler` turns an interface file (`name_vN.if`) into two crates: the
//! *interface* crate, with the types, the trait a component implements and
//! the export macro, and the *client* crate, with typed wrappers that call
//! the component's functions through their Invocations. Both use this crate
//! for the parts that do not depend on the interface.
//!
//! # Call convention
//!
//! Every interface function is one PPC export, called with
//! `Invocation.Call`. Its arguments are flattened into words (see [`Words`])
//! and passed in `x2..x7`; words the function does not use must be zero.
//! The function answers with two words through `Thread.Return`:
//!
//! | `r0` | `r1` |
//! |---|---|
//! | [`OUTCOME_OK`] | the result word, or zero without a result |
//! | `1..` | exception `r0 - 1` of the interface, in declaration order; its field word, or zero without a field |
//! | [`OUTCOME_MALFORMED`] | zero: the arguments did not decode |

pub use {
    libio_channel::IoRec,
    libobject::{
        CapError, RawKey,
        export::{PpcResult, init_return_key},
        invocation::InvocationKey,
        ppc_export,
        thread::ThreadReturnKey,
    },
};

/// Argument words of one call (`x2..x7`).
pub const ARGUMENT_WORDS: usize = 6;

/// `r0` of a call that succeeded.
pub const OUTCOME_OK: u64 = 0;

/// `r0` of a call whose arguments did not decode: a word out of its type's
/// range, or a nonzero unused word.
pub const OUTCOME_MALFORMED: u64 = u64::MAX;

/// A value that travels as a fixed number of call words.
///
/// Scalars take one word: unsigned values zero-extended, signed values
/// sign-extended, `bool` as 0 or 1, enumerations as their declaration index.
/// Records take their fields' words in declaration order. A reference to a
/// value in an xbuf data area takes two, its [`IoRec`] `base` and `len`.
pub trait Words: Sized {
    /// How many words the value takes.
    const COUNT: usize;

    /// Write the value into `words[..Self::COUNT]`.
    fn store(&self, words: &mut [u64]);

    /// Read a value from `words[..Self::COUNT]`; `None` when a word is out of
    /// range.
    fn load(words: &[u64]) -> Option<Self>;
}

macro_rules! unsigned_words {
    ($($unsigned:ty),*) => {$(
        impl Words for $unsigned {
            const COUNT: usize = 1;

            fn store(&self, words: &mut [u64]) {
                words[0] = u64::from(*self);
            }

            fn load(words: &[u64]) -> Option<Self> {
                Self::try_from(words[0]).ok()
            }
        }
    )*};
}

unsigned_words!(u8, u16, u32, u64);

macro_rules! signed_words {
    ($($signed:ty),*) => {$(
        impl Words for $signed {
            const COUNT: usize = 1;

            fn store(&self, words: &mut [u64]) {
                words[0] = i64::from(*self).cast_unsigned();
            }

            fn load(words: &[u64]) -> Option<Self> {
                Self::try_from(words[0].cast_signed()).ok()
            }
        }
    )*};
}

signed_words!(i8, i16, i32, i64);

impl Words for bool {
    const COUNT: usize = 1;

    fn store(&self, words: &mut [u64]) {
        words[0] = u64::from(*self);
    }

    fn load(words: &[u64]) -> Option<Self> {
        match words[0] {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }
    }
}

impl Words for IoRec {
    const COUNT: usize = 2;

    fn store(&self, words: &mut [u64]) {
        words[..2].copy_from_slice(&[self.base, self.len]);
    }

    fn load(words: &[u64]) -> Option<Self> {
        match *words {
            [base, len, ..] => Some(Self { base, len }),
            _ => None,
        }
    }
}

/// The answer of a call that succeeded with `result`.
pub const fn ok(result: u64) -> PpcResult {
    PpcResult {
        r0: OUTCOME_OK,
        r1: result,
    }
}

/// The answer of a call that raised exception `index` (declaration order in
/// its interface) carrying `field`.
pub const fn exception(index: u64, field: u64) -> PpcResult {
    PpcResult {
        r0: index + 1,
        r1: field,
    }
}

/// The answer of a call whose arguments did not decode.
pub const fn malformed() -> PpcResult {
    PpcResult {
        r0: OUTCOME_MALFORMED,
        r1: 0,
    }
}

/// Whether every argument word from `used` on is zero.
pub fn unused_words_are_zero(arguments: &[u64; ARGUMENT_WORDS], used: usize) -> bool {
    arguments.iter().skip(used).all(|&word| word == 0)
}

/// Proof that the target stacks of a client's bindings are used by one
/// Thread only, which makes calling through them safe.
pub struct ExclusiveStacks(());

impl ExclusiveStacks {
    /// The proof for a set of bindings.
    ///
    /// # Safety
    /// The target stack of every binding it is used with is reserved for the
    /// calling Thread: no other Thread calls through a binding with the same
    /// stack while this one does (see [`InvocationKey::call`]).
    pub const unsafe fn new() -> Self {
        Self(())
    }
}

/// One function a client may call: its Invocation and the target stack the
/// component reserved for this client's calls through it.
pub struct Binding {
    pub invocation: InvocationKey,
    pub stack_end: u64,
}

impl Binding {
    /// A binding of `invocation` with the target stack ending at `stack_end`.
    pub const fn new(invocation: InvocationKey, stack_end: u64) -> Self {
        Self {
            invocation,
            stack_end,
        }
    }

    /// Call the function with `arguments`, returning `(r0, r1)`.
    pub fn call(
        &self,
        _stacks: &ExclusiveStacks,
        arguments: [u64; ARGUMENT_WORDS],
    ) -> Result<(u64, u64), CapError> {
        // SAFETY: `ExclusiveStacks` proves this binding's stack is the
        // calling Thread's alone.
        unsafe { self.invocation.call(arguments, self.stack_end) }
    }
}

/// One function granted to a client: what Kickstart writes into the client's
/// init record for every function its `imports.if` uses.
#[repr(C)]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ImportRecord {
    /// Type code of the interface version the function belongs to
    pub type_code: u64,
    /// The function's declaration index in the interface
    pub function: u64,
    /// Wire key of the function's Invocation in the client's table
    pub invocation: u64,
    /// End of the target stack reserved for this client's calls
    pub stack_end: u64,
}

impl ImportRecord {
    /// The binding this record grants.
    pub const fn binding(&self) -> Binding {
        Binding::new(
            InvocationKey::from_key(RawKey::from_wire(self.invocation)),
            self.stack_end,
        )
    }
}

/// Export one interface function as a PPC entry named `$entry`.
///
/// `$dispatch` is the interface's generic decoder instantiated for the
/// implementing type, an `fn([u64; ARGUMENT_WORDS]) -> PpcResult`. Generated
/// export macros expand to one of these per function.
#[macro_export]
macro_rules! export_function {
    ($entry:ident, $dispatch:expr) => {
        const _: () = {
            #[allow(dead_code)]
            extern "C" fn body(
                _: u64,
                _: u64,
                input0: u64,
                input1: u64,
                input2: u64,
                input3: u64,
                input4: u64,
                input5: u64,
            ) -> $crate::PpcResult {
                ($dispatch)([input0, input1, input2, input3, input4, input5])
            }
    $crate::ppc_export!(#[allow(dead_code)] fn $entry => body);
        };
    };
}
