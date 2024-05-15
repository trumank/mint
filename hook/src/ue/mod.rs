mod array;
pub mod kismet;
mod malloc;
mod name;
mod object;
mod string;

pub use array::*;
pub use malloc::*;
pub use name::*;
pub use object::*;
pub use string::*;

use std::{ffi::c_void, ptr::NonNull};

use crate::globals;

pub type FnFFrameStep =
    unsafe extern "system" fn(stack: &mut kismet::FFrame, *mut UObject, result: *mut c_void);
pub type FnFFrameStepExplicitProperty = unsafe extern "system" fn(
    stack: &mut kismet::FFrame,
    result: *mut c_void,
    property: *const FProperty,
);
pub type FnFNameToString = unsafe extern "system" fn(&FName, &mut FString);
pub type FnFNameCtor = unsafe extern "system" fn(&mut FName, *const u16, EFindName);

pub type FnUObjectBaseUtilityGetPathName =
    unsafe extern "system" fn(NonNull<UObjectBase>, Option<NonNull<UObject>>, &mut FString);

#[derive(Debug, Clone, Copy)]
#[repr(u32)]
pub enum EFindName {
    Find = 0,
    Add = 1,
    ReplaceNotSafeForThreading = 2,
}

pub type FnStaticFindObjectFast = unsafe extern "system" fn(
    Option<NonNull<UClass>>,
    Option<NonNull<UObject>>,
    FName,
    bool,
    bool,
    EObjectFlags,
    EInternalObjectFlags,
) -> Option<NonNull<UObject>>;

pub fn static_find_object_fast(
    object_class: Option<NonNull<UClass>>,
    object_package: Option<NonNull<UObject>>,
    object_name: FName,
    exact_class: bool,
    any_package: bool,
    exclusive_flags: EObjectFlags,
    exclusive_internal_flags: EInternalObjectFlags,
) -> Option<NonNull<UObject>> {
    unsafe {
        (globals().static_find_object_fast())(
            object_class,
            object_package,
            object_name,
            exact_class,
            any_package,
            exclusive_flags,
            exclusive_internal_flags,
        )
    }
}

#[derive(Debug, Default)]
#[repr(C)]
pub struct FVector {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Debug, Default)]
#[repr(C)]
pub struct FLinearColor {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}
