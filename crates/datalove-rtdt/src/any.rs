//! Values whose type only a descriptor names.
//!
//! A generic function, in the runtime or in a rider, is handed each value as a
//! pointer and a descriptor, and the pointer has no Rust type to give it. These
//! give it one per thing the callee may do with the value: read it, change it
//! in place, take it over, or write one. Each is a pointer at the ABI, so a
//! caller in C or generated code passes the same word it always did, and the
//! descriptor still travels beside it as its own argument.
//!
//! They are not references. A reference to an opaque, zero-sized type is the
//! usual way to say "a pointer to something", but it grants access to no
//! bytes, and reading the value through it would read outside what it lends.
//! So each holds a raw pointer, never null, with a lifetime for the loan.
//!
//! Making one asks nothing, since nothing safe can be done with one; whoever
//! reads or writes through `as_ptr` takes the descriptor's word for what is
//! there.

use std::marker::PhantomData;
use std::mem::MaybeUninit;
use std::ptr::NonNull;

/// A value lent to be read: an `in` or `ref` parameter of a copyable type, or
/// a `ref` one of any type.
#[repr(transparent)]
#[derive(Copy, Clone)]
pub struct AnyRef<'a> {
    ptr: NonNull<u8>,
    _loan: PhantomData<&'a u8>,
}

/// A value lent to be changed in place: a `mut` parameter.
#[repr(transparent)]
pub struct AnyMut<'a> {
    ptr: NonNull<u8>,
    _loan: PhantomData<&'a mut u8>,
}

/// A value handed over: the callee moves it somewhere or destroys it, and the
/// caller does not use it again.
#[repr(transparent)]
pub struct AnyOwned<'a> {
    ptr: NonNull<u8>,
    _loan: PhantomData<&'a mut u8>,
}

/// Space to write a value into, uninitialized until the callee writes it: an
/// `out` parameter, or a result.
#[repr(transparent)]
pub struct AnyOut<'a> {
    ptr: NonNull<u8>,
    _loan: PhantomData<&'a mut u8>,
}

impl<'a> AnyRef<'a> {
    /// Lend a value of a type the descriptor passed with it names.
    #[inline]
    pub fn from_ref<T>(value: &'a T) -> Self {
        AnyRef { ptr: NonNull::from(value).cast(), _loan: PhantomData }
    }

    /// Wrap a pointer to a value.
    #[inline]
    pub fn new(ptr: *const u8) -> Self {
        AnyRef {
            ptr: NonNull::new(ptr.cast_mut()).expect("a value pointer is not null"),
            _loan: PhantomData,
        }
    }

    #[inline]
    pub fn as_ptr(self) -> *const u8 {
        self.ptr.as_ptr()
    }
}

macro_rules! exclusive {
    ($name:ident) => {
        impl $name<'_> {
            /// Wrap a pointer to a value.
            #[inline]
            pub fn new(ptr: *mut u8) -> Self {
                $name {
                    ptr: NonNull::new(ptr).expect("a value pointer is not null"),
                    _loan: PhantomData,
                }
            }

            #[inline]
            pub fn as_ptr(&self) -> *mut u8 {
                self.ptr.as_ptr()
            }

            /// Lend the value to be read, for as long as this is borrowed.
            #[inline]
            pub fn as_ref(&self) -> AnyRef<'_> {
                AnyRef { ptr: self.ptr, _loan: PhantomData }
            }
        }
    };
}

exclusive!(AnyMut);
exclusive!(AnyOwned);
exclusive!(AnyOut);

impl<'a> AnyMut<'a> {
    /// Lend a value to be changed, of a type the descriptor passed with it
    /// names.
    #[inline]
    pub fn from_mut<T>(value: &'a mut T) -> Self {
        AnyMut { ptr: NonNull::from(value).cast(), _loan: PhantomData }
    }
}

impl<'a> AnyOut<'a> {
    /// Lend space for a value of a type the descriptor passed with it names.
    #[inline]
    pub fn from_uninit<T>(space: &'a mut MaybeUninit<T>) -> Self {
        AnyOut { ptr: NonNull::from(space).cast(), _loan: PhantomData }
    }
}
