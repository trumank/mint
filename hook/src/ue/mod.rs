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

#[derive(Debug, Default, Clone, Copy)]
#[repr(C)]
pub struct FVector {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}
impl FVector {
    pub fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }
}
impl From<FVector> for nalgebra::Vector3<f32> {
    fn from(val: FVector) -> Self {
        nalgebra::Vector3::new(val.x, val.y, val.z)
    }
}
impl From<nalgebra::Vector3<f32>> for FVector {
    fn from(value: nalgebra::Vector3<f32>) -> Self {
        Self {
            x: value.x,
            y: value.y,
            z: value.z,
        }
    }
}
impl From<FVector> for nalgebra::Point3<f32> {
    fn from(val: FVector) -> Self {
        nalgebra::Point3::new(val.x, val.y, val.z)
    }
}
impl From<nalgebra::Point3<f32>> for FVector {
    fn from(value: nalgebra::Point3<f32>) -> Self {
        Self {
            x: value.x,
            y: value.y,
            z: value.z,
        }
    }
}

#[derive(Debug, Default, Copy, Clone)]
#[repr(C)]
pub struct FRotator {
    pub pitch: f32,
    pub yaw: f32,
    pub roll: f32,
}

#[derive(Debug, Default, Copy, Clone)]
#[repr(C)]
pub struct FLinearColor {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}
impl FLinearColor {
    pub fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }
}
