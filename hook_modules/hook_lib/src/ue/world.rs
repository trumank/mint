use element_ptr::element_ptr;
use std::{cell::UnsafeCell, ptr::NonNull};

use crate::util::{CastOptionNN as _, NN as _};

use super::{FLinearColor, FVector, TArray, UObject};

#[repr(C)]
pub struct UWorld {
    pub object: UObject,
    pub network_notify: *const (),
    pub persistent_level: *const (), // ULevel
    pub net_driver: *const (),       // UNetDriver
    pub line_batcher: *const ULineBatchComponent,
    pub persistent_line_batcher: *const ULineBatchComponent,
    pub foreground_line_batcher: *const ULineBatchComponent,

    padding: [u8; 0xc8],

    pub game_state: *const AGameStateBase,
    // TODO
}

#[repr(C)]
pub struct AGameStateBase {
    object: UObject,
    // TODO
}

#[repr(C)]
pub struct ULineBatchComponent {
    pub vftable: &'static ULineBatchComponentVTable,
    pub padding: UnsafeCell<[u8; 0x448]>,
    pub batched_lines: TArray<FBatchedLine>,
    pub batched_points: TArray<FBatchedPoint>,
    // lots more
}

#[repr(C)]
#[rustfmt::skip]
pub struct ULineBatchComponentVTable {
    pub padding: [*const (); 0x110],
    pub draw_line: extern "system" fn(this: &mut ULineBatchComponent, start: &FVector, end: &FVector, color: &FLinearColor, depth_priority: u8, life_time: f32, thickness: f32),
    pub draw_point: extern "system" fn(this: &mut ULineBatchComponent, position: &FVector, color: &FLinearColor, point_size: f32, depth_priority: u8, life_time: f32),
}

#[derive(Debug, Default, Copy, Clone)]
#[repr(C)]
pub struct FBatchedLine {
    pub start: FVector,
    pub end: FVector,
    pub color: FLinearColor,
    pub thickness: f32,
    pub remaining_life_time: f32,
    pub depth_priority: u8,
}
#[derive(Debug, Default, Copy, Clone)]
#[repr(C)]
pub struct FBatchedPoint {
    pub position: FVector,
    pub color: FLinearColor,
    pub point_size: f32,
    pub remaining_life_time: f32,
    pub depth_priority: u8,
}

pub unsafe fn get_world(mut ctx: Option<NonNull<UObject>>) -> Option<NonNull<UWorld>> {
    // TODO implement UEngine::GetWorldFromContextObject
    loop {
        let Some(outer) = ctx else {
            break;
        };
        let class = element_ptr!(outer => .uobject_base_utility.uobject_base.class_private.*).nn();
        if let Some(class) = class {
            // TODO can be done without allocation by creating and comparing FName instead
            if "/Script/Engine.World"
                == element_ptr!(class =>
                    .ustruct
                    .ufield
                    .uobject
                    .uobject_base_utility
                    .uobject_base.*)
                .get_path_name(None)
            {
                break;
            }
        }
        ctx = element_ptr!(outer => .uobject_base_utility.uobject_base.outer_private.*).nn();
    }
    ctx.cast()
}
