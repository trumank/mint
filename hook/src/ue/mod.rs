mod array;
pub mod kismet;
mod malloc;
mod map;
mod name;
mod object;
mod string;

pub use array::*;
pub use malloc::*;
pub use map::*;
pub use name::*;
pub use object::*;
pub use string::*;

use std::ffi::c_void;

use crate::globals;

pub type FnFFrameStep =
    unsafe extern "system" fn(stack: &mut kismet::FFrame, *mut UObject, result: *mut c_void);
pub type FnFFrameStepExplicitProperty = unsafe extern "system" fn(
    stack: &mut kismet::FFrame,
    result: *mut c_void,
    property: *const FProperty,
);
pub type FnFNameToString = unsafe extern "system" fn(&FName, &mut FString);
pub type FnFNameCtorWchar = unsafe extern "system" fn(&mut FName, *const u16, EFindName);

pub type FnUObjectBaseUtilityGetPathName =
    unsafe extern "system" fn(&UObjectBase, Option<&UObject>, &mut FString);

#[derive(Debug, Default)]
#[repr(C)]
pub struct FVector {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Debug, Default)]
#[repr(C)]
pub struct FRotator {
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
}

#[derive(Debug, Default)]
#[repr(C)]
pub struct FQuat {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

#[derive(Debug, Default)]
#[repr(C)]
pub struct FTransform {
    pub rotation: FQuat,
    pub translation: FVector,
    pub scale: FVector,
}

#[derive(Debug, Default)]
#[repr(C)]
pub struct FLinearColor {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}
