use {
    super::NucleusObject, crate::api::object_type::ObjectType, core::ptr::NonNull,
    libsyscall::CapError,
};

// ═══════════════════════════════════════════════════════════════════
// TYPE-ERASED OBJECT POINTER
// ═══════════════════════════════════════════════════════════════════

/// A type-erased pointer to a kernel object, with its type tag.
///
/// This is the "fat pointer" alternative - we store the type alongside
/// the pointer so we can safely cast it back.
#[derive(Clone, Copy)]
pub struct ObjectRef {
    ptr: NonNull<()>,
    obj_type: ObjectType,
}

impl ObjectRef {
    /// Create a new object reference from a typed pointer
    pub fn new<T: NucleusObject>(obj: &T) -> Self {
        Self {
            ptr: NonNull::from(obj).cast(),
            obj_type: T::TYPE,
        }
    }

    /// Create from a mutable pointer (for objects in pools)
    ///
    /// # Safety
    /// Caller must ensure the pointer is valid and properly aligned
    pub unsafe fn from_raw<T: NucleusObject>(ptr: *mut T) -> Self {
        Self {
            ptr: NonNull::new_unchecked(ptr.cast()),
            obj_type: T::TYPE,
        }
    }

    /// Get the object type
    #[inline]
    pub fn object_type(&self) -> ObjectType {
        self.obj_type
    }

    /// Attempt to cast to a specific type (immutable)
    #[inline]
    pub fn try_as<T: NucleusObject>(&self) -> Option<&T> {
        if self.obj_type == T::TYPE {
            // SAFETY: We verified the type matches
            Some(unsafe { self.ptr.cast::<T>().as_ref() })
        } else {
            None
        }
    }

    /// Attempt to cast to a specific type (mutable)
    #[inline]
    pub fn try_as_mut<T: NucleusObject>(&mut self) -> Option<&mut T> {
        if self.obj_type == T::TYPE {
            // SAFETY: We verified the type matches
            Some(unsafe { self.ptr.cast::<T>().as_mut() })
        } else {
            None
        }
    }

    /// Cast with error on type mismatch
    #[inline]
    pub fn as_type<T: NucleusObject>(&self) -> Result<&T, CapError> {
        self.try_as().ok_or(CapError::TypeMismatch {
            expected: T::TYPE,
            found: self.obj_type,
        })
    }

    /// Cast with error on type mismatch (mutable)
    #[inline]
    pub fn as_type_mut<T: NucleusObject>(&mut self) -> Result<&mut T, CapError> {
        self.try_as_mut().ok_or(CapError::TypeMismatch {
            expected: T::TYPE,
            found: self.obj_type,
        })
    }
}
