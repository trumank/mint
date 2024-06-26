use std::ffi::c_void;

use crate::globals;

#[repr(C)]
pub struct TArray<T> {
    data: *mut T,
    num: i32,
    max: i32,
}
impl<T: std::fmt::Debug> std::fmt::Debug for TArray<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.as_slice().fmt(f)
    }
}
impl<T> TArray<T> {
    pub fn new() -> Self {
        Self {
            data: std::ptr::null_mut(),
            num: 0,
            max: 0,
        }
    }
    pub fn as_ptr(&self) -> *const T {
        self.data
    }
    pub fn as_mut_ptr(&mut self) -> *mut T {
        self.data
    }
}
impl<T> Drop for TArray<T> {
    fn drop(&mut self) {
        unsafe {
            std::ptr::drop_in_place(std::ptr::slice_from_raw_parts_mut(
                self.data,
                self.num as usize,
            ))
        }
        globals().gmalloc().free(self.data.cast());
    }
}
impl<T> Default for TArray<T> {
    fn default() -> Self {
        Self {
            data: std::ptr::null_mut(),
            num: 0,
            max: 0,
        }
    }
}
impl<T> TArray<T> {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            data: globals().gmalloc().malloc(
                capacity * std::mem::size_of::<T>(),
                std::mem::align_of::<T>() as u32,
            ) as *mut _,
            num: 0,
            max: capacity as i32,
        }
    }
    pub fn len(&self) -> usize {
        self.num as usize
    }
    pub fn capacity(&self) -> usize {
        self.max as usize
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn as_slice(&self) -> &[T] {
        if self.num == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(self.data, self.num as usize) }
        }
    }
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        if self.num == 0 {
            &mut []
        } else {
            unsafe { std::slice::from_raw_parts_mut(self.data as *mut _, self.num as usize) }
        }
    }
    pub fn clear(&mut self) {
        let elems: *mut [T] = self.as_mut_slice();

        unsafe {
            self.num = 0;
            std::ptr::drop_in_place(elems);
        }
    }
    pub fn reserve(&mut self, additional: usize) {
        if self.num + additional as i32 >= self.max {
            self.max = u32::next_power_of_two((self.max + additional as i32) as u32) as i32;
            let new = globals().gmalloc().realloc(
                self.data as *mut c_void,
                self.max as usize * std::mem::size_of::<T>(),
                std::mem::align_of::<T>() as u32,
            ) as *mut _;
            self.data = new;
        }
    }
    pub fn push(&mut self, new_value: T) {
        self.reserve(1);
        unsafe {
            std::ptr::write(self.data.add(self.num as usize), new_value);
        }
        self.num += 1;
    }
    pub fn extend_from_slice(&mut self, other: &[T])
    where
        T: Copy,
    {
        self.reserve(other.len());
        // TODO optimize
        for elm in other {
            self.push(*elm);
        }
    }
}

impl<T> From<&[T]> for TArray<T>
where
    T: Copy,
{
    fn from(value: &[T]) -> Self {
        let mut new = Self::with_capacity(value.len());
        new.extend_from_slice(value);
        new
    }
}
