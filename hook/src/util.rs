use element_ptr::element_ptr;
use std::ptr::NonNull;

use crate::ue;

pub trait NN<T> {
    fn nn(self) -> Option<NonNull<T>>;
}
impl<T> NN<T> for *const T {
    fn nn(self) -> Option<NonNull<T>> {
        NonNull::new(self.cast_mut())
    }
}
impl<T> NN<T> for *mut T {
    fn nn(self) -> Option<NonNull<T>> {
        NonNull::new(self)
    }
}
pub trait CastOptionNN<T, O> {
    fn cast(self) -> Option<NonNull<O>>;
}
impl<T, O> CastOptionNN<T, O> for Option<NonNull<T>> {
    fn cast(self) -> Option<NonNull<O>> {
        self.map(|s| s.cast())
    }
}
