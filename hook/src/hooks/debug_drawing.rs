use std::ffi::c_void;
use std::ptr::NonNull;

use element_ptr::element_ptr;
use na::{Matrix, Matrix4, Point3, Vector3};
use nalgebra as na;

use crate::hooks::ExecFn;
use crate::ue::{self, FLinearColor, FRotator, FVector, TArray, UObject};

pub fn kismet_hooks() -> &'static [(&'static str, ExecFn)] {
    &[
        (
            "/Script/Engine.KismetSystemLibrary:DrawDebugLine",
            exec_draw_debug_line as ExecFn,
        ),
        (
            "/Script/Engine.KismetSystemLibrary:DrawDebugPoint",
            exec_draw_debug_point as ExecFn,
        ),
        (
            "/Script/Engine.KismetSystemLibrary:DrawDebugCircle",
            exec_draw_debug_circle as ExecFn,
        ),
        (
            "/Script/Engine.KismetSystemLibrary:DrawDebugSphere",
            exec_draw_debug_sphere as ExecFn,
        ),
        (
            "/Script/Engine.KismetSystemLibrary:DrawDebugCone",
            exec_draw_debug_cone as ExecFn,
        ),
        (
            "/Script/Engine.KismetSystemLibrary:DrawDebugConeInDegrees",
            exec_draw_debug_cone_in_degrees as ExecFn,
        ),
        (
            "/Script/Engine.KismetSystemLibrary:DrawDebugCylinder",
            exec_draw_debug_cylinder as ExecFn,
        ),
        (
            "/Script/Engine.KismetSystemLibrary:DrawDebugCapsule",
            exec_draw_debug_capsule as ExecFn,
        ),
        (
            "/Script/Engine.KismetSystemLibrary:DrawDebugBox",
            exec_draw_debug_box as ExecFn,
        ),
        (
            "/Game/_AssemblyStorm/TestMod/DebugStuff.DebugStuff_C:ReceiveTick",
            exec_tick as ExecFn,
        ),
        (
            "/Game/_mint/BPL_CSG.BPL_CSG_C:Get Procedural Mesh Vertices",
            exec_get_mesh_vertices as ExecFn,
        ),
    ]
}
#[repr(C)]
struct UWorld {
    object: ue::UObject,
    network_notify: *const (),
    persistent_level: *const (), // ULevel
    net_driver: *const (),       // UNetDriver
    line_batcher: *const ULineBatchComponent,
    persistent_line_batcher: *const ULineBatchComponent,
    foreground_line_batcher: *const ULineBatchComponent,

    padding: [u8; 0xc8],

    game_state: *const AGameStateBase,
    // TODO
}

#[repr(C)]
struct AGameStateBase {
    object: ue::UObject,
    // TODO
}

#[repr(C)]
struct AFSDGameState {
    object: ue::UObject,

    padding: [u8; 0x3f8],

    csg_world: *const nav::ADeepCSGWorld,
    // TODO
}

#[cfg(test)]
mod test {
    use super::*;

    const _: [u8; 0x420] = [0; std::mem::offset_of!(AFSDGameState, csg_world)];
    //const _: [u8; 0x128] = [0; std::mem::size_of::<FDeepCellStored>()];
}

#[repr(C)]
struct ULineBatchComponent {
    vftable: *const ULineBatchComponentVTable,
    padding: [u8; 0x448],
    batched_lines: TArray<FBatchedLine>,
    batched_points: TArray<FBatchedPoint>,
    // lots more
}

#[repr(C)]
#[rustfmt::skip]
struct ULineBatchComponentVTable {
    padding: [*const (); 0x110],
    draw_line: unsafe extern "system" fn(this: NonNull<ULineBatchComponent>, start: &FVector, end: &FVector, color: &FLinearColor, depth_priority: u8, life_time: f32, thickness: f32),
    draw_point: unsafe extern "system" fn(this: NonNull<ULineBatchComponent>, position: &FVector, color: &FLinearColor, point_size: f32, depth_priority: u8, life_time: f32),
}

#[derive(Debug, Default, Copy, Clone)]
#[repr(C)]
struct FBatchedLine {
    start: FVector,
    end: FVector,
    color: FLinearColor,
    thickness: f32,
    remaining_life_time: f32,
    depth_priority: u8,
}
#[derive(Debug, Default, Copy, Clone)]
#[repr(C)]
struct FBatchedPoint {
    position: FVector,
    color: FLinearColor,
    point_size: f32,
    remaining_life_time: f32,
    depth_priority: u8,
}

unsafe fn get_batcher(world: NonNull<UWorld>, duration: f32) -> NonNull<ULineBatchComponent> {
    if duration > 0. {
        element_ptr!(world => .persistent_line_batcher.*)
    } else {
        element_ptr!(world => .line_batcher.*)
    }
    .nn()
    .unwrap()
}

unsafe fn draw_lines(batcher: NonNull<ULineBatchComponent>, lines: &[FBatchedLine]) {
    if let Some((last, lines_)) = lines.split_last() {
        let batched_lines: &mut TArray<_> = element_ptr!(batcher => .batched_lines).as_mut();
        batched_lines.extend_from_slice(lines_);

        // call draw_line directly on last element so it gets properly marked as dirty
        let draw_line = element_ptr!(batcher => .vftable.*.draw_line.*);
        draw_line(
            batcher,
            &last.start,
            &last.end,
            &last.color,
            last.depth_priority,
            last.thickness,
            last.remaining_life_time,
        );
    }
}

unsafe fn draw_points(batcher: NonNull<ULineBatchComponent>, lines: &[FBatchedPoint]) {
    if let Some((last, lines)) = lines.split_last() {
        let batched_points: &mut TArray<_> = element_ptr!(batcher => .batched_points).as_mut();
        batched_points.extend_from_slice(lines);

        // call draw_point directly on last element so it gets properly marked as dirty
        let draw_point = element_ptr!(batcher => .vftable.*.draw_point.*);
        draw_point(
            batcher,
            &last.position,
            &last.color,
            last.point_size,
            last.depth_priority,
            last.remaining_life_time,
        );
    }
}

trait NN<T> {
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
trait CastOptionNN<T, O> {
    fn cast(self) -> Option<NonNull<O>>;
}
impl<T, O> CastOptionNN<T, O> for Option<NonNull<T>> {
    fn cast(self) -> Option<NonNull<O>> {
        self.map(|s| s.cast())
    }
}

unsafe fn get_world(mut ctx: Option<NonNull<ue::UObject>>) -> Option<NonNull<UWorld>> {
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

unsafe extern "system" fn exec_draw_debug_line(
    _context: *mut ue::UObject,
    stack: *mut ue::kismet::FFrame,
    _result: *mut c_void,
) {
    let stack = stack.as_mut().unwrap();

    let world_context: Option<NonNull<ue::UObject>> = stack.arg();
    let start: FVector = stack.arg();
    let end: FVector = stack.arg();
    let color: FLinearColor = stack.arg();
    let duration: f32 = stack.arg();
    let thickness: f32 = stack.arg();

    if let Some(world) = get_world(world_context) {
        let batcher = get_batcher(world, duration);
        let f = element_ptr!(batcher => .vftable.*.draw_line.*);
        f(batcher, &start, &end, &color, 0, thickness, duration);
    }

    if !stack.code.is_null() {
        stack.code = stack.code.add(1);
    }
}

unsafe extern "system" fn exec_draw_debug_point(
    _context: *mut ue::UObject,
    stack: *mut ue::kismet::FFrame,
    _result: *mut c_void,
) {
    let stack = stack.as_mut().unwrap();

    let world_context: Option<NonNull<ue::UObject>> = stack.arg();
    let position: FVector = stack.arg();
    let size: f32 = stack.arg();
    let color: FLinearColor = stack.arg();
    let duration: f32 = stack.arg();

    if let Some(world) = get_world(world_context) {
        let batcher = get_batcher(world, duration);
        let f = element_ptr!(batcher => .vftable.*.draw_point.*);
        f(batcher, &position, &color, size, 0, duration);
    }

    if !stack.code.is_null() {
        stack.code = stack.code.add(1);
    }
}

unsafe extern "system" fn exec_draw_debug_circle(
    _context: *mut ue::UObject,
    stack: *mut ue::kismet::FFrame,
    _result: *mut c_void,
) {
    let stack = stack.as_mut().unwrap();

    let world_context: Option<NonNull<ue::UObject>> = stack.arg();
    let center: FVector = stack.arg();
    let radius: f32 = stack.arg();
    let num_segments: u32 = stack.arg();
    let color: FLinearColor = stack.arg();
    let duration: f32 = stack.arg();
    let thickness: f32 = stack.arg();
    let y_axis: FVector = stack.arg();
    let z_axis: FVector = stack.arg();
    let draw_axis: bool = stack.arg();

    if let Some(world) = get_world(world_context) {
        let batcher = get_batcher(world, duration);

        let line_config = FBatchedLine {
            color,
            remaining_life_time: duration,
            thickness,
            ..Default::default()
        };

        let mut tm = Matrix4::identity();
        tm.fixed_view_mut::<3, 1>(0, 3).copy_from(&center.into());

        let x_axis = Vector3::new(1.0, 0.0, 0.0);
        tm.fixed_view_mut::<3, 1>(0, 0).copy_from(&x_axis);
        tm.fixed_view_mut::<3, 1>(0, 1).copy_from(&y_axis.into());
        tm.fixed_view_mut::<3, 1>(0, 2).copy_from(&z_axis.into());

        let mut segments = num_segments.max(4);
        let angle_step = 2.0 * std::f32::consts::PI / segments as f32;

        let center = get_origin(&tm);
        let axis_y = Vector3::new(tm[(0, 1)], tm[(1, 1)], tm[(2, 1)]);
        let axis_z = Vector3::new(tm[(0, 2)], tm[(1, 2)], tm[(2, 2)]);

        let mut lines = Vec::with_capacity(segments as usize);

        let mut angle: f32 = 0.0;
        while segments > 0 {
            let vertex1 = center + radius * (axis_y * angle.cos() + axis_z * angle.sin());
            angle += angle_step;
            let vertex2 = center + radius * (axis_y * angle.cos() + axis_z * angle.sin());
            lines.push(FBatchedLine {
                start: vertex1.into(),
                end: vertex2.into(),
                ..line_config
            });
            segments -= 1;
        }

        if draw_axis {
            lines.push(FBatchedLine {
                start: (center - radius * axis_y).into(),
                end: (center + radius * axis_y).into(),
                ..line_config
            });
            lines.push(FBatchedLine {
                start: (center - radius * axis_z).into(),
                end: (center + radius * axis_z).into(),
                ..line_config
            });
        }
        draw_lines(batcher, &lines);
    }

    if !stack.code.is_null() {
        stack.code = stack.code.add(1);
    }
}

unsafe extern "system" fn exec_draw_debug_sphere(
    _context: *mut ue::UObject,
    stack: *mut ue::kismet::FFrame,
    _result: *mut c_void,
) {
    let stack = stack.as_mut().unwrap();

    let world_context: Option<NonNull<ue::UObject>> = stack.arg();
    let center: FVector = stack.arg();
    let radius: f32 = stack.arg();
    let num_segments: u32 = stack.arg();
    let color: FLinearColor = stack.arg();
    let duration: f32 = stack.arg();
    let thickness: f32 = stack.arg();

    if let Some(world) = get_world(world_context) {
        let batcher = get_batcher(world, duration);

        let line_config = FBatchedLine {
            color,
            remaining_life_time: duration,
            thickness,
            ..Default::default()
        };

        let segments = num_segments.max(4);

        let angle_inc = 2.0 * std::f32::consts::PI / segments as f32;
        let mut num_segments_y = segments;
        let mut latitude = angle_inc;
        let mut sin_y1 = 0.0;
        let mut cos_y1 = 1.0;
        let center: Vector3<f32> = center.into();

        let mut lines = Vec::with_capacity(num_segments_y as usize * segments as usize * 2);

        while num_segments_y > 0 {
            let sin_y2 = latitude.sin();
            let cos_y2 = latitude.cos();

            let mut vertex1 = Vector3::new(sin_y1, 0.0, cos_y1) * radius + center;
            let mut vertex3 = Vector3::new(sin_y2, 0.0, cos_y2) * radius + center;
            let mut longitude = angle_inc;

            let mut num_segments_x = segments;
            while num_segments_x > 0 {
                let sin_x = longitude.sin();
                let cos_x = longitude.cos();

                let vertex2 =
                    Vector3::new(cos_x * sin_y1, sin_x * sin_y1, cos_y1) * radius + center;
                let vertex4 =
                    Vector3::new(cos_x * sin_y2, sin_x * sin_y2, cos_y2) * radius + center;

                lines.push(FBatchedLine {
                    start: vertex1.into(),
                    end: vertex2.into(),
                    ..line_config
                });
                lines.push(FBatchedLine {
                    start: vertex1.into(),
                    end: vertex3.into(),
                    ..line_config
                });

                vertex1 = vertex2;
                vertex3 = vertex4;
                longitude += angle_inc;
                num_segments_x -= 1;
            }

            sin_y1 = sin_y2;
            cos_y1 = cos_y2;
            latitude += angle_inc;
            num_segments_y -= 1;
        }

        draw_lines(batcher, &lines);
    }

    if !stack.code.is_null() {
        stack.code = stack.code.add(1);
    }
}

fn find_best_axis_vectors(direction: &Vector3<f32>) -> (Vector3<f32>, Vector3<f32>) {
    let nx = direction.x.abs();
    let ny = direction.y.abs();
    let nz = direction.z.abs();

    let axis1 = if nz > nx && nz > ny {
        Vector3::new(1., 0., 0.)
    } else {
        Vector3::new(0., 0., 1.)
    };

    let tmp = axis1 - direction * direction.dot(&axis1);
    let axis1_normalized = tmp.normalize();

    (axis1_normalized, axis1_normalized.cross(direction))
}

fn get_origin<T: Copy>(
    matrix: &Matrix<T, na::Const<4>, nalgebra::Const<4>, na::ArrayStorage<T, 4, 4>>,
) -> Vector3<T> {
    Vector3::new(matrix[(0, 3)], matrix[(1, 3)], matrix[(2, 3)])
}

fn add_half_circle(
    lines: &mut Vec<FBatchedLine>,
    base: &Vector3<f32>,
    x: &Vector3<f32>,
    y: &Vector3<f32>,
    color: &FLinearColor,
    radius: f32,
    num_sides: i32,
    life_time: f32,
    depth_priority: u8,
    thickness: f32,
) {
    let num_sides = num_sides.max(2);
    let angle_delta = 2.0 * std::f32::consts::PI / num_sides as f32;
    let mut last_vertex = base + x * radius;

    for side_index in 0..(num_sides / 2) {
        let i = (side_index + 1) as f32;
        let vertex = base + (x * (angle_delta * i).cos() + y * (angle_delta * i).sin()) * radius;
        lines.push(FBatchedLine {
            start: last_vertex.into(),
            end: vertex.into(),
            color: *color,
            remaining_life_time: life_time,
            thickness,
            depth_priority,
        });
        last_vertex = vertex;
    }
}

fn add_circle(
    lines: &mut Vec<FBatchedLine>,
    base: &Vector3<f32>,
    x: &Vector3<f32>,
    y: &Vector3<f32>,
    color: &FLinearColor,
    radius: f32,
    num_sides: i32,
    life_time: f32,
    depth_priority: u8,
    thickness: f32,
) {
    let num_sides = num_sides.max(2);
    let angle_delta = 2.0 * std::f32::consts::PI / num_sides as f32;
    let mut last_vertex = base + x * radius;

    for side_index in 0..num_sides {
        let i = (side_index + 1) as f32;
        let vertex = base + (x * (angle_delta * i).cos() + y * (angle_delta * i).sin()) * radius;
        lines.push(FBatchedLine {
            start: last_vertex.into(),
            end: vertex.into(),
            color: *color,
            remaining_life_time: life_time,
            thickness,
            depth_priority,
        });
        last_vertex = vertex;
    }
}

unsafe fn draw_cone(
    batcher: NonNull<ULineBatchComponent>,
    origin: FVector,
    direction: FVector,
    length: f32,
    angle_width: f32,
    angle_height: f32,
    num_sides: u32,
    color: FLinearColor,
    duration: f32,
    thickness: f32,
) {
    let line_config = FBatchedLine {
        color,
        remaining_life_time: duration,
        thickness,
        ..Default::default()
    };

    let origin: Vector3<f32> = origin.into();
    let direction: Vector3<f32> = direction.into();

    let num_sides = num_sides.max(4) as usize;

    let angle1 = angle_height.clamp(f32::EPSILON, std::f32::consts::PI - f32::EPSILON);
    let angle2 = angle_width.clamp(f32::EPSILON, std::f32::consts::PI - f32::EPSILON);

    let sin_x_2 = (0.5 * angle1).sin();
    let sin_y_2 = (0.5 * angle2).sin();

    let sin_sq_x_2 = sin_x_2 * sin_x_2;
    let sin_sq_y_2 = sin_y_2 * sin_y_2;

    let mut cone_verts = Vec::with_capacity(num_sides);

    for i in 0..num_sides {
        let fraction = i as f32 / num_sides as f32;
        let thi = 2.0 * std::f32::consts::PI * fraction;
        let phi = (thi.sin() * sin_y_2).atan2(thi.cos() * sin_x_2);
        let sin_phi = phi.sin();
        let cos_phi = phi.cos();
        let sin_sq_phi = sin_phi * sin_phi;
        let cos_sq_phi = cos_phi * cos_phi;

        let r_sq = sin_sq_x_2 * sin_sq_y_2 / (sin_sq_x_2 * sin_sq_phi + sin_sq_y_2 * cos_sq_phi);
        let r = r_sq.sqrt();
        let sqr = (1.0 - r_sq).sqrt();
        let alpha = r * cos_phi;
        let beta = r * sin_phi;

        cone_verts.push(Vector3::new(
            1.0 - 2.0 * r_sq,
            2.0 * sqr * alpha,
            2.0 * sqr * beta,
        ));
    }

    let direction_norm = direction.normalize();
    let (y_axis, z_axis) = find_best_axis_vectors(&direction_norm);
    let cone_to_world = Matrix4::from_columns(&[
        direction_norm.push(0.),
        y_axis.push(0.),
        z_axis.push(0.),
        origin.push(1.),
    ]) * Matrix4::new_scaling(length);

    let mut lines = vec![];

    let mut current_point = Vector3::zeros();
    let mut prev_point = Vector3::zeros();
    let mut first_point = Vector3::zeros();
    for (i, vert) in cone_verts.iter().enumerate().take(num_sides) {
        current_point = cone_to_world.transform_point(&(*vert).into()).coords;
        lines.push(FBatchedLine {
            start: get_origin(&cone_to_world).into(),
            end: current_point.into(),
            ..line_config
        });

        if i > 0 {
            lines.push(FBatchedLine {
                start: prev_point.into(),
                end: current_point.into(),
                ..line_config
            });
        } else {
            first_point = current_point;
        }

        prev_point = current_point;
    }
    lines.push(FBatchedLine {
        start: current_point.into(),
        end: first_point.into(),
        ..line_config
    });

    draw_lines(batcher, &lines);
}

unsafe extern "system" fn exec_draw_debug_cone(
    _context: *mut ue::UObject,
    stack: *mut ue::kismet::FFrame,
    _result: *mut c_void,
) {
    let stack = stack.as_mut().unwrap();

    let world_context: Option<NonNull<ue::UObject>> = stack.arg();
    let origin: FVector = stack.arg();
    let direction: FVector = stack.arg();
    let length: f32 = stack.arg();
    let angle_width: f32 = stack.arg();
    let angle_height: f32 = stack.arg();
    let num_sides: u32 = stack.arg();
    let color: FLinearColor = stack.arg();
    let duration: f32 = stack.arg();
    let thickness: f32 = stack.arg();

    if let Some(world) = get_world(world_context) {
        let batcher = get_batcher(world, duration);
        draw_cone(
            batcher,
            origin,
            direction,
            length,
            angle_width,
            angle_height,
            num_sides,
            color,
            duration,
            thickness,
        );
    }

    if !stack.code.is_null() {
        stack.code = stack.code.add(1);
    }
}
unsafe extern "system" fn exec_draw_debug_cone_in_degrees(
    _context: *mut ue::UObject,
    stack: *mut ue::kismet::FFrame,
    _result: *mut c_void,
) {
    let stack = stack.as_mut().unwrap();

    let world_context: Option<NonNull<ue::UObject>> = stack.arg();
    let origin: FVector = stack.arg();
    let direction: FVector = stack.arg();
    let length: f32 = stack.arg();
    let angle_width: f32 = stack.arg();
    let angle_height: f32 = stack.arg();
    let num_sides: u32 = stack.arg();
    let color: FLinearColor = stack.arg();
    let duration: f32 = stack.arg();
    let thickness: f32 = stack.arg();

    if let Some(world) = get_world(world_context) {
        let batcher = get_batcher(world, duration);
        draw_cone(
            batcher,
            origin,
            direction,
            length,
            angle_width.to_radians(),
            angle_height.to_radians(),
            num_sides,
            color,
            duration,
            thickness,
        );
    }

    if !stack.code.is_null() {
        stack.code = stack.code.add(1);
    }
}

unsafe extern "system" fn exec_draw_debug_cylinder(
    _context: *mut ue::UObject,
    stack: *mut ue::kismet::FFrame,
    _result: *mut c_void,
) {
    let stack = stack.as_mut().unwrap();

    let world_context: Option<NonNull<ue::UObject>> = stack.arg();
    let start: FVector = stack.arg();
    let end: FVector = stack.arg();
    let radius: f32 = stack.arg();
    let segments: u32 = stack.arg();
    let color: FLinearColor = stack.arg();
    let duration: f32 = stack.arg();
    let thickness: f32 = stack.arg();

    if let Some(world) = get_world(world_context) {
        let batcher = get_batcher(world, duration);

        let line_config = FBatchedLine {
            color,
            remaining_life_time: duration,
            thickness,
            ..Default::default()
        };

        let mut segments = segments.max(4);

        let end: Vector3<f32> = end.into();
        let start: Vector3<f32> = start.into();

        let angle_inc = 360.0 / segments as f32;
        let mut angle = angle_inc;

        let mut axis = (end - start).normalize();
        if axis == Vector3::zeros() {
            axis = Vector3::new(0.0, 0.0, 1.0);
        }

        let (perpendicular, _) = find_best_axis_vectors(&axis);

        let offset = perpendicular * radius;

        let mut p1 = start + offset;
        let mut p3 = end + offset;

        let mut lines = vec![];
        while segments > 0 {
            let rotation =
                na::Rotation3::from_axis_angle(&na::Unit::new_normalize(axis), angle.to_radians());
            let p2 = start + rotation.transform_vector(&offset);
            let p4 = end + rotation.transform_vector(&offset);

            lines.push(FBatchedLine {
                start: p2.into(),
                end: p4.into(),
                ..line_config
            });
            lines.push(FBatchedLine {
                start: p1.into(),
                end: p2.into(),
                ..line_config
            });
            lines.push(FBatchedLine {
                start: p3.into(),
                end: p4.into(),
                ..line_config
            });

            p1 = p2;
            p3 = p4;
            angle += angle_inc;
            segments -= 1;
        }

        draw_lines(batcher, &lines);
    }

    if !stack.code.is_null() {
        stack.code = stack.code.add(1);
    }
}

unsafe extern "system" fn exec_draw_debug_capsule(
    _context: *mut ue::UObject,
    stack: *mut ue::kismet::FFrame,
    _result: *mut c_void,
) {
    let stack = stack.as_mut().unwrap();

    let world_context: Option<NonNull<ue::UObject>> = stack.arg();
    let center: FVector = stack.arg();
    let half_height: f32 = stack.arg();
    let radius: f32 = stack.arg();
    let rotation: FRotator = stack.arg();
    let color: FLinearColor = stack.arg();
    let duration: f32 = stack.arg();
    let thickness: f32 = stack.arg();

    if let Some(world) = get_world(world_context) {
        let batcher = get_batcher(world, duration);

        let line_config = FBatchedLine {
            color,
            remaining_life_time: duration,
            thickness,
            ..Default::default()
        };

        let mut lines = vec![];

        const DRAW_COLLISION_SIDES: i32 = 16;
        let origin: Vector3<f32> = center.into();
        let rot = na::Rotation3::from_euler_angles(
            rotation.roll.to_radians(),
            rotation.pitch.to_radians(),
            rotation.yaw.to_radians(),
        );
        let axes = rot.matrix();

        let x_axis = axes.fixed_view::<3, 1>(0, 0).xyz();
        let y_axis = axes.fixed_view::<3, 1>(0, 1).xyz();
        let z_axis = axes.fixed_view::<3, 1>(0, 2).xyz();

        // Draw top and bottom circles
        let half_axis = (half_height - radius).max(1.0);
        let top_end = origin + (half_axis * z_axis);
        let bottom_end = origin - half_axis * z_axis;

        add_circle(
            &mut lines,
            &top_end,
            &x_axis,
            &y_axis,
            &color,
            radius,
            DRAW_COLLISION_SIDES,
            duration,
            0,
            thickness,
        );
        add_circle(
            &mut lines,
            &bottom_end,
            &x_axis,
            &y_axis,
            &color,
            radius,
            DRAW_COLLISION_SIDES,
            duration,
            0,
            thickness,
        );

        // Draw domed caps
        add_half_circle(
            &mut lines,
            &top_end,
            &y_axis,
            &z_axis,
            &color,
            radius,
            DRAW_COLLISION_SIDES,
            duration,
            0,
            thickness,
        );
        add_half_circle(
            &mut lines,
            &top_end,
            &x_axis,
            &z_axis,
            &color,
            radius,
            DRAW_COLLISION_SIDES,
            duration,
            0,
            thickness,
        );

        let neg_z_axis = -z_axis;

        add_half_circle(
            &mut lines,
            &bottom_end,
            &y_axis,
            &neg_z_axis,
            &color,
            radius,
            DRAW_COLLISION_SIDES,
            duration,
            0,
            thickness,
        );
        add_half_circle(
            &mut lines,
            &bottom_end,
            &x_axis,
            &neg_z_axis,
            &color,
            radius,
            DRAW_COLLISION_SIDES,
            duration,
            0,
            thickness,
        );

        // Draw connected lines
        lines.push(FBatchedLine {
            start: (top_end + radius * x_axis).into(),
            end: (bottom_end + radius * x_axis).into(),
            ..line_config
        });
        lines.push(FBatchedLine {
            start: (top_end - radius * x_axis).into(),
            end: (bottom_end - radius * x_axis).into(),
            ..line_config
        });
        lines.push(FBatchedLine {
            start: (top_end + radius * y_axis).into(),
            end: (bottom_end + radius * y_axis).into(),
            ..line_config
        });
        lines.push(FBatchedLine {
            start: (top_end - radius * y_axis).into(),
            end: (bottom_end - radius * y_axis).into(),
            ..line_config
        });

        draw_lines(batcher, &lines);
    }

    if !stack.code.is_null() {
        stack.code = stack.code.add(1);
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct DebugBox {
    center: FVector,
    extent: FVector,
    color: FLinearColor,
    rotation: FRotator,
    duration: f32,
    thickness: f32,
}
impl DebugBox {
    unsafe fn draw(&self, lines: &mut Vec<FBatchedLine>) {
        let Self {
            center,
            extent,
            color,
            rotation,
            duration,
            thickness,
        } = *self;

        let line_config = FBatchedLine {
            color,
            remaining_life_time: duration,
            thickness,
            ..Default::default()
        };

        let center: Vector3<f32> = center.into();
        let extent: Vector3<f32> = extent.into();

        let transform = na::Isometry3::from_parts(
            na::Translation3::from(center),
            na::Rotation3::from_euler_angles(
                rotation.roll.to_radians(),
                rotation.pitch.to_radians(),
                rotation.yaw.to_radians(),
            )
            .into(),
        );

        let half_dimensions: Vector3<f32> = extent * 0.5;

        let vertices = [
            Point3::new(half_dimensions.x, half_dimensions.y, half_dimensions.z),
            Point3::new(half_dimensions.x, -half_dimensions.y, half_dimensions.z),
            Point3::new(-half_dimensions.x, -half_dimensions.y, half_dimensions.z),
            Point3::new(-half_dimensions.x, half_dimensions.y, half_dimensions.z),
            Point3::new(half_dimensions.x, half_dimensions.y, -half_dimensions.z),
            Point3::new(half_dimensions.x, -half_dimensions.y, -half_dimensions.z),
            Point3::new(-half_dimensions.x, -half_dimensions.y, -half_dimensions.z),
            Point3::new(-half_dimensions.x, half_dimensions.y, -half_dimensions.z),
            Point3::new(half_dimensions.x, half_dimensions.y, half_dimensions.z),
            Point3::new(half_dimensions.x, half_dimensions.y, -half_dimensions.z),
            Point3::new(half_dimensions.x, -half_dimensions.y, half_dimensions.z),
            Point3::new(half_dimensions.x, -half_dimensions.y, -half_dimensions.z),
            Point3::new(-half_dimensions.x, -half_dimensions.y, half_dimensions.z),
            Point3::new(-half_dimensions.x, -half_dimensions.y, -half_dimensions.z),
            Point3::new(-half_dimensions.x, half_dimensions.y, half_dimensions.z),
            Point3::new(-half_dimensions.x, half_dimensions.y, -half_dimensions.z),
        ];

        let indices = [
            (0, 1),
            (1, 2),
            (2, 3),
            (3, 0),
            (4, 5),
            (5, 6),
            (6, 7),
            (7, 4),
            (0, 4),
            (1, 5),
            (2, 6),
            (3, 7),
        ];

        for &(i, j) in &indices {
            lines.push(FBatchedLine {
                start: (transform * vertices[i]).into(),
                end: (transform * vertices[j]).into(),
                ..line_config
            });
        }
    }
}

unsafe extern "system" fn exec_draw_debug_box(
    _context: *mut ue::UObject,
    stack: *mut ue::kismet::FFrame,
    _result: *mut c_void,
) {
    let stack = stack.as_mut().unwrap();

    let world_context: Option<NonNull<ue::UObject>> = stack.arg();
    let shape = DebugBox {
        center: stack.arg(),
        extent: stack.arg(),
        color: stack.arg(),
        rotation: stack.arg(),
        duration: stack.arg(),
        thickness: stack.arg(),
    };

    if let Some(world) = get_world(world_context) {
        let batcher = element_ptr!(world => .line_batcher.*).nn().unwrap();
        let mut lines = vec![];
        shape.draw(&mut lines);
        draw_lines(batcher, &lines);
    }

    if !stack.code.is_null() {
        stack.code = stack.code.add(1);
    }
}

unsafe extern "system" fn exec_tick(
    context: *mut ue::UObject,
    stack: *mut ue::kismet::FFrame,
    _result: *mut c_void,
) {
    let stack = stack.as_mut().unwrap();

    let _delta_seconds: f32 = stack.arg();

    if let Some(world) = get_world(context.nn()) {
        nav::render(world);
    }

    if !stack.code.is_null() {
        stack.code = stack.code.add(1);
    }
}

#[derive(Debug, Clone)]
#[repr(C)]
pub struct UDeepProceduralMeshComponent {
    padding: [u8; 0x488],
    chunk_id: nav::FChunkId,
    triangle_mesh: *const physx::PxTriangleMesh,
}

mod physx {
    use super::*;

    #[repr(C)]
    #[rustfmt::skip]
    pub struct PxTriangleVTable {
        padding: [u8; 0x28],
        pub get_nb_vertices: unsafe extern "system" fn(this: NonNull<PxTriangleMesh>) -> u32,
        pub get_vertices: unsafe extern "system" fn(this: NonNull<PxTriangleMesh>) -> *const FVector,
        pub get_vertices_for_modification: unsafe extern "system" fn(this: NonNull<PxTriangleMesh>) -> *const FVector,
        pub refit_bvh: *const (),
        pub get_nb_triangles: unsafe extern "system" fn(this: NonNull<PxTriangleMesh>) -> u32,
        pub get_triangles: unsafe extern "system" fn(this: NonNull<PxTriangleMesh>) -> *const (),
    }

    #[repr(C)]
    pub struct PxTriangleMesh {
        pub vftable: NonNull<PxTriangleVTable>,
        // TODO rest
    }
}

unsafe extern "system" fn exec_get_mesh_vertices(
    _context: *mut ue::UObject,
    stack: *mut ue::kismet::FFrame,
    _result: *mut c_void,
) {
    let stack = stack.as_mut().unwrap();

    let mesh: Option<NonNull<UDeepProceduralMeshComponent>> = stack.arg();
    let _world_context: Option<NonNull<UObject>> = stack.arg();

    drop(stack.arg::<TArray<FVector>>());
    let ret: &mut TArray<FVector> = &mut *(stack.most_recent_property_address as *mut _);
    *ret = TArray::new();

    if let Some(mesh) = mesh {
        if let Some(triangle_mesh) = element_ptr!(mesh => .triangle_mesh.*).nn() {
            let num = element_ptr!(triangle_mesh => .vftable.*.get_nb_vertices.*)(triangle_mesh);
            let ptr = element_ptr!(triangle_mesh => .vftable.*.get_vertices.*)(triangle_mesh);
            let slice = std::slice::from_raw_parts(ptr, num as usize);
            let c = element_ptr!(mesh => .chunk_id.*);

            for vert in slice {
                let position = FVector::new(
                    c.x as f32 * 800. + vert.x,
                    c.y as f32 * 800. + vert.y,
                    c.z as f32 * 800. + vert.z,
                );
                ret.push(position);
            }
        }
    }

    if !stack.code.is_null() {
        stack.code = stack.code.add(1);
    }
}

mod nav {
    use self::ue::TArray;

    use super::*;

    pub unsafe fn draw_box(
        lines: &mut Vec<FBatchedLine>,
        center: FVector,
        extent: FVector,
        color: FLinearColor,
    ) {
        DebugBox {
            center,
            extent,
            color,
            rotation: FRotator {
                pitch: 0.,
                yaw: 0.,
                roll: 0.,
            },
            duration: 0.,
            thickness: 2.,
        }
        .draw(lines);
    }

    unsafe fn get_path(
        pathfinder: NonNull<DeepPathfinder>,
        start: &FVector,
        end: &FVector,
    ) -> Option<Vec<FVector>> {
        let get_path: FnGetPath = std::mem::transmute(0x143dae9b0 as usize);

        let mut path = vec![];

        let mut start = *start;

        loop {
            let mut tmp = TArray::default();
            let mut complete = false;
            let res = get_path(
                pathfinder,
                DeepPathFinderType::Walk,
                DeepPathFinderSize::Small,
                DeepPathFinderPreference::None,
                &start,
                end,
                &mut tmp,
                &mut complete,
            );
            if res != EPathfinderResult::Success {
                return None;
            }
            path.extend_from_slice(tmp.as_slice());
            if complete {
                return Some(path);
            }
            start = *path.last().unwrap();
        }
    }

    unsafe fn path_stuff(
        lines: &mut Vec<FBatchedLine>,
        points: &mut Vec<FBatchedPoint>,
        csg_world: NonNull<ADeepCSGWorld>,
    ) {
        //println!("csg_world {csg_world:?}");
        //let nav = element_ptr!(csg_world => .active_nav_data.*.nav_sets5);
        //let nodes = element_ptr!(nav => .nodes.*);
        //let connections = element_ptr!(nav => .connections);

        if let Some(pathfinder) = element_ptr!(csg_world => .pathfinder.*).nn() {
            for i in -5..=5 {
                let path = get_path(
                    pathfinder,
                    &FVector::new(1000.0, i as f32 * 100.0, 0.),
                    &FVector::new(-1000.0, i as f32 * 100.0, 0.),
                );
                //println!("{} {:?}", complete, res);
                if let Some(path) = path {
                    let mut iter = path.as_slice().iter().peekable();
                    while let Some(start) = iter.next() {
                        if let Some(end) = iter.peek() {
                            lines.push(FBatchedLine {
                                start: *start,
                                end: **end,
                                color: FLinearColor::new(1., 0., 0., 1.),
                                thickness: 10.,
                                ..Default::default()
                            })
                        }
                    }
                }
            }
            /*
            let mut spawn_points = TArray::default();
            get_all_spawn_points(
                pathfinder,
                DeepPathFinderType::Walk,
                DeepPathFinderSize::Small,
                &FVector::default(),
                250.0,
                &mut spawn_points,
            );

            println!("len = {}", spawn_points.len());
            for point in spawn_points.as_slice() {
                draw_box(
                    &mut lines,
                    *point,
                    FVector::new(30., 30., 30.),
                    FLinearColor::new(0., 0., 1., 1.),
                );
            }
            */
        }

        let mut i: u8 = 0;
        //for n in 0..(nodes.count as usize) {
        //    let node = element_ptr!(nodes.start => + (n).*);
        //    let pos = node.pathfinder_pos.chunk_id_and_offset;
        for x in 0..1 {
            for y in 0..1 {
                for z in 0..1 {
                    let pos = FChunkIDAndOffset {
                        chunk_id: FChunkId {
                            x: x / 4,
                            y: y / 4,
                            z: z / 4,
                        },
                        offset: FChunkOffset {
                            x: x % 4,
                            y: y % 4,
                            z: z % 4,
                        },
                    };

                    i += 1;

                    let rank = 0; //node.rank
                    let c = if rank == 0 {
                        FLinearColor::new(0., 1., 0., 0.5)
                    } else if rank == 1 {
                        FLinearColor::new(0., 0., 1., 0.5)
                    } else {
                        FLinearColor::new(1., 0., 0., 0.5)
                    };

                    let get_cell_real: FnGetCellReal = std::mem::transmute(0x143dc2ff0 as usize);
                    let get_cell_server_real: FnGetCellServerReal =
                        std::mem::transmute(0x143dc30b0 as usize);

                    let core = element_ptr!(csg_world => .core_world.*).nn().unwrap();
                    let cell = {
                        let a = get_cell(core, pos);
                        let b = get_cell_real(core, NonNull::from(&pos));
                        assert_eq!(a, b);
                        a
                    };
                    let cell_server = {
                        let a = get_cell_server(core, pos);
                        let b = get_cell_server_real(core, NonNull::from(&pos));
                        assert_eq!(a, b);
                        a
                    };

                    //let c = if let Some(cell) = cell {
                    //    let cell = element_ptr!(cell => .*);
                    //    //println!(
                    //    //    "{:?} {:?}",
                    //    //    node.pathfinder_pos.chunk_id_and_offset, cell.bounding_box
                    //    //);
                    //    FLinearColor::new(cell.solidity as f32 / 255., 0., 0., 0.5)
                    //    //FLinearColor::new(
                    //    //    cell.tmp_solidity.max(cell.solidity) as f32 / 255.,
                    //    //    0.,
                    //    //    0.,
                    //    //    0.5,
                    //    //)
                    //} else {
                    //    c
                    //};

                    let red = FLinearColor::new(1., 0., 0., 0.5);
                    let blue = FLinearColor::new(0., 0., 1., 0.5);
                    let black = FLinearColor::new(0., 0., 0., 0.5);
                    let c = if let Some(cell) = cell_server {
                        let cell = element_ptr!(cell => .*);

                        //let pmv = cell_server.prevent_spawn_material_volumn.volume1;
                        //if pmv != 0 {
                        //    println!(
                        //        "{:?} {:?}",
                        //        pos.to_world_pos(),
                        //        cell_server.prevent_spawn_material_volumn
                        //    );
                        //}

                        //println!("{:032b}", cell_server.danger_material_volume);
                        //if cell_server.danger_material_volume.volume1 != 0 {
                        //    red
                        //} else if cell_server.prevent_spawn_material_volumn.volume1 != 0 {
                        //    let index = cell_server.prevent_spawn_material_volumn.volume1;

                        //    let buffer = element_ptr!(core => .array_pools.*).BitVolumeBuffers1;
                        //    let elm =
                        //        element_ptr!(buffer.inner.Pool.start => + (index as usize).*);
                        //    println!("{:?} {:?}", pos.to_world_pos(), elm);

                        //    blue
                        //} else {
                        //    black
                        //}
                        //println!();
                        if cell.prevent_spawn_material_volume.volume1 != 0 {
                            let index = cell.prevent_spawn_material_volume.volume1;
                            let buffer = element_ptr!(core => .array_pools.*).BitVolumeBuffers1;
                            let elm = element_ptr!(buffer.inner.Pool.start => + (index as usize).*);
                            //println!("{i:02X} SPAWN volume1 {:02X?}", elm.data);
                        }
                        if cell.prevent_spawn_material_volume.volume2 != 0 {
                            let index = cell.prevent_spawn_material_volume.volume2;
                            let buffer = element_ptr!(core => .array_pools.*).BitVolumeBuffers2;
                            let elm = element_ptr!(buffer.inner.Pool.start => + (index as usize).*);
                            //println!("{i:02X} SPAWN volume2 {:02X?}", elm.data);
                        }
                        if cell.prevent_spawn_material_volume.volume3 != 0 {
                            let index = cell.prevent_spawn_material_volume.volume3;
                            let buffer = element_ptr!(core => .array_pools.*).BitVolumeBuffers3;
                            let elm = element_ptr!(buffer.inner.Pool.start => + (index as usize).*);
                            //println!("{i:02X} SPAWN volume3 {:02X?}", elm.data);
                            let r = 8;
                            for x in 0..r {
                                for y in 0..r {
                                    for z in 0..r {
                                        let d = (elm.data[y + r * z] >> x) & 1;
                                        if d == 0 {
                                            continue;
                                        }

                                        let w = pos.to_world_pos();
                                        let position = FVector {
                                            x: w.x + (2. * x as f32 + 1.) / r as f32 * 100. - 100.,
                                            y: w.y + (2. * y as f32 + 1.) / r as f32 * 100. - 100.,
                                            z: w.z + (2. * z as f32 + 1.) / r as f32 * 100. - 100.,
                                        };
                                        points.push(FBatchedPoint {
                                            position,
                                            color: red,
                                            point_size: 40.,
                                            remaining_life_time: 0.,
                                            depth_priority: 0,
                                        })
                                        //draw_box(
                                        //    lines,
                                        //    FVector {
                                        //        x: w.x + (2. * x as f32 + 1.) / r as f32 * 100.
                                        //            - 100.,
                                        //        y: w.y + (2. * y as f32 + 1.) / r as f32 * 100.
                                        //            - 100.,
                                        //        z: w.z + (2. * z as f32 + 1.) / r as f32 * 100.
                                        //            - 100.,
                                        //    },
                                        //    FVector::new(20., 20., 20.),
                                        //    FLinearColor::new(0., d as f32 / 256., 0., 1.),
                                        //);
                                    }
                                }
                            }
                        }
                        if cell.danger_material_volume.volume1 != 0 {
                            let index = cell.danger_material_volume.volume1;
                            let buffer = element_ptr!(core => .array_pools.*).BitVolumeBuffers1;
                            let elm = element_ptr!(buffer.inner.Pool.start => + (index as usize).*);
                            //println!("{i:02X} DANGER volume1 {:02X?}", elm.data);
                        }
                        if cell.danger_material_volume.volume2 != 0 {
                            let index = cell.danger_material_volume.volume2;
                            let buffer = element_ptr!(core => .array_pools.*).BitVolumeBuffers2;
                            let elm = element_ptr!(buffer.inner.Pool.start => + (index as usize).*);
                            //println!("{i:02X} DANGER volume2 {:02X?}", elm.data);
                        }
                        if cell.danger_material_volume.volume3 != 0 {
                            let index = cell.danger_material_volume.volume3;
                            let buffer = element_ptr!(core => .array_pools.*).BitVolumeBuffers3;
                            let elm = element_ptr!(buffer.inner.Pool.start => + (index as usize).*);
                            //println!("{i:02X} DANGER volume3 {:02X?}", elm.data);
                        }
                        black
                        /*
                        if cell.sdf_volume.volume3 != 0 {
                            let index = cell.sdf_volume.volume3;

                            let buffer = element_ptr!(core => .array_pools.*).VolumeBuffers3;
                            let elm =
                                element_ptr!(buffer.inner.Pool.start => + (index as usize).*);

                            //println!("{:?} {:08b?}", pos.to_world_pos(), elm);
                            //println!("{:?}", elm.data);

                            let r = 8;
                            for x in 0..r {
                                for y in 0..r {
                                    for z in 0..r {
                                        let d = elm.data[x + r * (y + r * z)];
                                        if d == 0 {
                                            continue;
                                        }

                                        let w = pos.to_world_pos();
                                        draw_box(
                                            lines,
                                            FVector {
                                                x: w.x + (2. * x as f32 + 1.) / r as f32 * 100.
                                                    - 100.,
                                                y: w.y + (2. * y as f32 + 1.) / r as f32 * 100.
                                                    - 100.,
                                                z: w.z + (2. * z as f32 + 1.) / r as f32 * 100.
                                                    - 100.,
                                            },
                                            FVector::new(20., 20., 20.),
                                            FLinearColor::new(0., d as f32 / 256., 0., 1.),
                                        );
                                    }
                                }
                            }

                            red
                        } else {
                            black
                        }
                        */
                    } else {
                        black
                    };

                    draw_box(
                        lines,
                        pos.to_world_pos(),
                        FVector {
                            x: 180.,
                            y: 180.,
                            z: 180.,
                        },
                        c,
                    );

                    //println!("{:?}", elm);
                    //nodes.start[]
                }
            }
        }

        //let vtable = element_ptr!(csg_world => .object.uobject_base_utility.uobject_base.vtable.*);
        //println!("tick {csg_world:?} {}", nodes.count);
    }

    unsafe fn nav_stuff(
        lines: &mut Vec<FBatchedLine>,
        points: &mut Vec<FBatchedPoint>,
        csg_world: NonNull<ADeepCSGWorld>,
    ) {
        let nav = element_ptr!(csg_world => .active_nav_data.*.nav_sets5);
        let nodes = element_ptr!(nav => .nodes.*);
        let connections = element_ptr!(nav => .connections);
        for n in 0..(nodes.count as usize) {
            let node = element_ptr!(nodes.start => + (n).*);
            let w = node.pathfinder_pos.chunk_id_and_offset.to_world_pos();
            let s = node.pathfinder_pos;
            let r = 8;
            draw_box(
                lines,
                FVector {
                    x: w.x + (2. * s.sub_x as f32 + 1.) / r as f32 * 100. - 100.,
                    y: w.y + (2. * s.sub_y as f32 + 1.) / r as f32 * 100. - 100.,
                    z: w.z + (2. * s.sub_z as f32 + 1.) / r as f32 * 100. - 100.,
                },
                FVector::new(10., 10., 10.),
                FLinearColor::new(0., 1., 0., 1.),
            )
        }
    }

    pub unsafe fn render(world: NonNull<UWorld>) {
        let batcher = element_ptr!(world => .line_batcher.*).nn().unwrap();

        let mut lines = vec![];
        let mut points = vec![];

        //let get_all_spawn_points: FnGetAllSpawnPointsInSphere = std::mem::transmute(0x143dc28a0 as usize);

        let game_state = element_ptr!(world => .game_state.* as AFSDGameState);
        let csg_world = element_ptr!(game_state => .csg_world.*).nn();
        if let Some(csg_world) = csg_world {
            //println!("csg_world {csg_world:?}");

            path_stuff(&mut lines, &mut points, csg_world);
            //nav_stuff(&mut lines, &mut points, csg_world);
        }

        draw_lines(batcher, &lines);
        draw_points(batcher, &points);
    }

    #[derive(Debug, Clone)]
    #[repr(C)]
    pub struct FSDVirtualMemAllocator {
        padding: [u8; 0x40],
    }

    #[derive(Debug, Clone, Copy)]
    #[repr(C)]
    pub struct FChunkId {
        pub x: i16,
        pub y: i16,
        pub z: i16,
    }

    #[derive(Debug, Clone, Copy)]
    #[repr(C)]
    pub struct FChunkOffset {
        pub x: i16,
        pub y: i16,
        pub z: i16,
    }

    #[derive(Debug, Clone, Copy)]
    #[repr(C)]
    pub struct FChunkIDAndOffset {
        chunk_id: FChunkId,
        offset: FChunkOffset,
    }
    impl FChunkIDAndOffset {
        fn to_world_pos(&self) -> FVector {
            let c = &self.chunk_id;
            let o = &self.offset;

            let x = c.x as f32 * 8. + o.x as f32 * 2.;
            let y = c.y as f32 * 8. + o.y as f32 * 2.;
            let z = c.z as f32 * 8. + o.z as f32 * 2.;

            FVector {
                x: x * 100. + 100.,
                y: y * 100. + 100.,
                z: z * 100. + 100.,
            }
        }
    }

    #[derive(Debug, Clone)]
    #[repr(C)]
    pub struct FPathfinderCoordinate {
        chunk_id_and_offset: FChunkIDAndOffset,
        sub_x: u8,
        sub_y: u8,
        sub_z: u8,
        padding: u8,
    }

    #[derive(Debug, Clone)]
    #[repr(C)]
    pub struct NodeIdx {
        id: i32,
    }

    #[derive(Debug, Clone)]
    #[repr(C)]
    pub struct ConnectionIdx {
        id: i32,
    }

    #[derive(Debug, Clone)]
    #[repr(C)]
    pub struct FDeepNavSetFNode {
        pathfinder_pos: FPathfinderCoordinate,
        parent: NodeIdx,
        rank: u16,
        first_connection: ConnectionIdx,
        num_calls: u16,
    }
    impl FDeepNavSetFNode {
        fn to_world_pos(&self) -> FVector {
            self.pathfinder_pos.chunk_id_and_offset.to_world_pos()
        }
    }

    #[derive(Debug, Clone)]
    #[repr(C)]
    pub struct FDeepNavSetFConnection {
        node: NodeIdx,
        next_connection: ConnectionIdx,
    }

    #[derive(Debug, Clone)]
    #[repr(C)]
    pub struct DeepVirtExpandingArray<T> {
        start: *const T,
        count: i32,
        allocated: i32,
        virtual_mem: FSDVirtualMemAllocator,
    }

    #[derive(Debug, Clone)]
    #[repr(C)]
    pub struct FDeepNavSet {
        nodes: DeepVirtExpandingArray<FDeepNavSetFNode>,
        connections: DeepVirtExpandingArray<FDeepNavSetFConnection>,
    }

    #[derive(Debug, Clone)]
    #[repr(C)]
    pub struct FDeepNavData {
        nav_sets1: FDeepNavSet,
        nav_sets2: FDeepNavSet,
        nav_sets3: FDeepNavSet,
        nav_sets4: FDeepNavSet,
        nav_sets5: FDeepNavSet,
        nav_sets6: FDeepNavSet,
    }

    #[derive(Debug, Clone)]
    #[repr(C)]
    pub struct ADeepCSGWorld {
        padding: [u8; 0x6e0],
        core_world: *const FCoreCSGWorld,
        terrain_scheduler: u64,
        create_debris: bool,
        pathfinder: *const DeepPathfinder,
        active_nav_data: *const FDeepNavData,
        next_nav_data: *const FDeepNavData,
    }

    //#[repr(C)]
    //pub struct ADeepCSGWorld {
    //    object: ue::UObject,
    //}

    #[derive(Debug)]
    #[repr(C)]
    pub struct FCoreCSGWorld {
        init_net_section: [u8; 0x40],
        handle_to_material: [u8; 0x40],
        default_scanner_material: [u8; 0x40],
        unknown: [u8; 0x40],
        array_pools: FDeepArrayPools,
        section_infos: DeepSparseArray<FDeepSectionInfo>,
        cells: DeepVirtExpandingArray<FDeepCellStored>,
        cells_connectivity: DeepVirtExpandingArray<FDeepCellStoredConnectivity>,
        cells_pathfinder: DeepVirtExpandingArray<FDeepCellStoredServer>,
        edge_cells: [[u8; 0x128]; 0x40], //FDeepCellStored[0x40]
        padding: [u8; 0x4a00],
        sections_allocated: DeepBitArray, //DeepBitArray<5242880> SectionsAllocated;
        alloc_server_cells: bool,
    }
    #[derive(Debug, Clone)]
    #[repr(C)]
    pub struct DeepBitArray {
        high_bits: [u64; 0x500],
        bits: [u64; 0x14000],
    }
    #[derive(Debug, Clone)]
    #[repr(C)]
    pub struct DeepSparseArray<T> {
        num_alloced: i32,
        buffer: *const T,
        virtual_mem: [u8; 0x40], // FSDVirtualMemAllocator
    }
    #[derive(Debug, Clone)]
    #[repr(C)]
    pub struct FDeepSectionInfo {
        connectivity: [u8; 0x18], // FDeepChunkStoredConnectivity
        section_actor: *const (), // ADeepCSGSection*
        triangle_mesh: *const (), // physx::PxTriangleMesh*
        cell_offset: u32,
        has_no_triangles: u8,
        normal_zmin: u8,
        normal_zmax: u8,
    }

    #[derive(Debug)]
    #[repr(C)]
    pub struct FDeepArrayPools {
        Planes: DeepArrayPool<FDeepCSGPlane, FDeepCellStored>,
        SubMeshes: DeepArrayPool<FSubMeshInfo, FDeepCellStored>,
        SubVolumes: DeepArrayPool<FSubVolumeInfo, FDeepCellStored>,
        VertexPositions: DeepArrayPool<FTerrainMeshVertex, FDeepCellStored>,
        Faces: DeepArrayPool<FTerrainMeshFace, FDeepCellStored>,
        PhysTriangles: DeepArrayPool<FTerrainPhysTriangle, FDeepCellStored>,
        Debris: DeepArrayPool<FDebrisCSGPoint, FDeepCellStored>,
        AttachPoints: DeepArrayPool<FAttachCSGPoint, FDeepCellStored>,
        FPNodes: DeepArrayPool<PFCellNode, FDeepCellStoredServer>,
        PFCollision: DeepArrayPool<FPFCollisionKey, FDeepCellStored>,
        ConnectivityPoints: DeepArrayPool<FTerrainConnectivityPoint, FDeepCellStoredConnectivity>,
        ConnectivitySidePoints:
            DeepArrayPool<FTerrainSideConnectivity, FDeepCellStoredConnectivity>,
        InternalConnectivityUF:
            DeepArrayPool<FChunkInternalConnectivityUnionFind, FDeepCellStoredConnectivity>,
        GlobalConnectivityUF:
            DeepArrayPool<FChunkGlobalConnectivityUnionFind, FDeepChunkStoredConnectivity>,
        VolumeBuffers1: DeepVolumeBufferPool<0x8>,
        BitVolumeBuffers1: DeepBitVolumeBufferPool<0x4>,
        VolumeBuffers2: DeepVolumeBufferPool<0x40>,
        BitVolumeBuffers2: DeepBitVolumeBufferPool<0x10>,
        VolumeBuffers3: DeepVolumeBufferPool<0x200>,
        BitVolumeBuffers3: DeepBitVolumeBufferPool<0x40>,
    }

    #[cfg(test)]
    mod test {
        use super::*;
        const _: [u8; 0x1438 - 0x10a0] = [0; std::mem::size_of::<DeepVolumeBufferPool<0x8>>()];
        const _: [u8; 0x1ad0 - 0x1438] = [0; std::mem::size_of::<DeepBitVolumeBufferPool<0x4>>()];
        const _: [u8; 0x130] =
            [0; std::mem::size_of::<DeepArrayPool<FDeepCSGPlane, FDeepCellStored>>()];
        const _: [u8; 0xaed78] = [0; std::mem::size_of::<FCoreCSGWorld>()];
        const _: [u8; 0x2f30] = [0; std::mem::size_of::<FDeepArrayPools>()];

        const _: [u8; 0x6f8] = [0; std::mem::offset_of!(ADeepCSGWorld, pathfinder)];

        const _: [u8; 0xc4] = [0; std::mem::offset_of!(FDeepCellStored, mesh_bounding_box)];
        const _: [u8; 0x3038] = [0; std::mem::offset_of!(FCoreCSGWorld, section_infos) + 8];
        const _: [u8; 0xc570] = [0; std::mem::offset_of!(FCoreCSGWorld, sections_allocated)];
        const _: [u8; 0x3080] = [0; std::mem::offset_of!(FCoreCSGWorld, cells)];
        const _: [u8; 0x128] = [0; std::mem::size_of::<FDeepCellStored>()];
        const _: [u8; 0x1c0] = [0; std::mem::size_of::<FDeepCellStoredServer>()];

        const _: [u8; 0x450] = [0; std::mem::offset_of!(ULineBatchComponent, batched_lines)];
        const _: [u8; 0x34] = [0; std::mem::size_of::<FBatchedLine>()];
    }

    #[derive(Debug, Clone)]
    #[repr(C)]
    struct FDeepChunkStoredConnectivity {
        /* offset 0x000 */ singleSideChunkConnectivity: [u8; 6],
        /* offset 0x006 */ numChunkConnectivityRegions: u8,
        /* offset 0x008 */ ConnectivyUF0: FChunkGlobalConnectivityUnionFind,
        /* offset 0x014 */
        ConnectivityUFs: DeepArrayPool<FChunkGlobalConnectivityUnionFind, u32>,
        //DeepArrayPool<FChunkGlobalConnectivityUnionFind,FDeepChunkStoredConnectivity *,33554432,4096>::RangeIdx
    }

    #[derive(Debug, Clone)]
    #[repr(C)]
    struct FEncodedChunkId {
        /* offset 0x000 */ id: u32,
    }

    #[derive(Debug, Clone)]
    #[repr(C)]
    struct FChunkGlobalConnectivityId {
        /* offset 0x000 */ ChunkId: FEncodedChunkId,
        /* offset 0x004 */ ChunkIdx: u8,
    }

    #[derive(Debug, Clone)]
    #[repr(C)]
    struct FChunkGlobalConnectivityUnionFind {
        /* offset 0x000 */ Parent: FChunkGlobalConnectivityId,
        /* offset 0x008 */ Rank: u16,
    }

    #[derive(Debug, Clone)]
    #[repr(C)]
    struct FChunkInternalConnectivityId {
        /* offset 0x000 */ CellId: u8,
        /* offset 0x001 */ SolidIdx: u8,
    }

    #[derive(Debug, Clone)]
    #[repr(C)]
    struct FChunkInternalConnectivityUnionFind {
        /* offset 0x000 */ Parent: FChunkInternalConnectivityId,
        /* offset 0x002 */ Rank: u16,
        /* offset 0x004 */ ChunkIndex: u8,
    }

    #[derive(Debug, Clone)]
    #[repr(C)]
    struct FTerrainSideConnectivity {
        /* offset 0x000 */ SolidIdx: u8,
        /* offset 0x001 */ SideIdx: u8,
    }

    #[derive(Debug, Clone)]
    #[repr(C)]
    struct FTerrainConnectivityPoint {
        /* offset 0x000 */ Point: FVector,
        /* offset 0x00c */ SolidIdx: u8,
        /* offset 0x00d */ SideIdx: u8,
    }

    #[derive(Debug, Clone)]
    #[repr(C)]
    struct FPFCollisionKey {
        /* offset 0x000 */ val: u32,
    }

    #[derive(Debug, Clone)]
    #[repr(C)]
    struct PFCellNode {
        // union
        /* offset 0x000 */ //LocalRoot: Type0x98a6d /* TODO: figure out how to name it */,
        /* offset 0x000 */ //NumCells: Type0x98a6f /* TODO: figure out how to name it */,
        /* offset 0x000 */ //NumSpawnCells: Type0x98a71 /* TODO: figure out how to name it */,
        val: u32,
        /* offset 0x004 */ GlobalNode: u32,
    }

    #[derive(Debug, Clone)]
    #[repr(C)]
    struct FAttachCSGPoint {
        /* offset 0x000 */ Pos: FVector,
        /* offset 0x00c */ Index: i32,
    }

    #[derive(Debug, Clone)]
    #[repr(C)]
    struct FDebrisCSGPoint {
        /* offset 0x000 */ Pos: FVector,
        /* offset 0x00c */ Index: i32,
        /* offset 0x010 */ DebrisComponent: *const (), // UDebrisInstances,
    }

    #[derive(Debug, Clone)]
    #[repr(C)]
    struct FTerrainPhysTriangle {
        /* offset 0x000 */ BaseIdx: u32,
        /* offset 0x004 */ Offset1: u8,
        /* offset 0x005 */ Offset2: u8,
        /* offset 0x006 */ Material: u16,
    }

    #[derive(Debug, Clone)]
    #[repr(C)]
    struct FTerrainMeshFace {
        /* offset 0x000 */ Normal: [u8; 3],
        /* offset 0x003 */ VertCount: u8,
    }

    #[derive(Debug, Clone)]
    #[repr(C)]
    pub struct FWindowsCriticalSection {
        padding: [u8; 0x28],
    }

    #[derive(Debug, Clone)]
    #[repr(C)]
    pub struct PFTypes_FSectionBuffer<const N: usize> {
        data: [u8; N],
    }

    #[derive(Debug, Clone)]
    #[repr(C)]
    pub struct PFTypes_FBitBuffer<const N: usize> {
        data: [u8; N],
    }

    #[derive(Debug)]
    #[repr(C)]
    pub struct DeepVolumeBufferPool<const N: usize> {
        inner: DeepArenaPool<PFTypes_FSectionBuffer<N>, 128>,
    }

    #[derive(Debug)]
    #[repr(C)]
    pub struct DeepBitVolumeBufferPool<const N: usize> {
        inner: DeepArenaPool<PFTypes_FBitBuffer<N>, 256>,
    }

    #[derive(Debug)]
    #[repr(C)]
    pub struct TArrayInline<T, const N: usize> {
        inline_data: [T; N],
        secondary_data: *const T,
        num: i32,
        max: i32,
    }

    #[derive(Debug)]
    #[repr(C)]
    pub struct DeepArenaPool<T, const A: usize> {
        Pool: DeepVirtExpandingArray<T>,
        FreeIndices: TArrayInline<u32, A>, // TODO inline allocator
        RefCounts: TArrayInline<u16, A>,   // TODO inline allocator
        CriticalSection: FWindowsCriticalSection,
    }

    #[derive(Debug, Clone)]
    #[repr(C)]
    struct DeepArrayPoolRange<C> {
        start: u32,
        end: u32,
        handle: *const C,
    }

    #[derive(Debug, Clone)]
    #[repr(u32)]
    enum GarbState {
        NotRunning = 0x0,
        Running = 0x1,
    }

    #[derive(Debug, Clone)]
    #[repr(C)]
    pub struct DeepArrayPool<T, C> {
        /* offset 0x000 */ Pool: DeepVirtExpandingArray<T>,
        /* offset 0x050 */
        ActiveRangeList: [DeepVirtExpandingArray<DeepArrayPoolRange<C>>; 2],
        /* offset 0x0f0 */ ActiveRange: u32,
        /* offset 0x0f4 */ GState: GarbState,
        /* offset 0x0f8 */ GarbIndex: i32,
        /* offset 0x0fc */ GarbLastEnd: u32,
        /* offset 0x100 */ GarbageAmount: i32,
        /* offset 0x108 */ CriticalSection: FWindowsCriticalSection,
    }

    #[derive(Debug, Clone)]
    #[repr(C)]
    pub struct FDeepCSGPlane {
        /* offset 0x000 */ plane: FPackedPlane,
        /* offset 0x008 */ top: FDeepCSGNode,
        /* offset 0x00c */ bottom: FDeepCSGNode,
    }

    #[derive(Debug, Clone)]
    #[repr(C)]
    struct FPackedPlane {
        /* offset 0x000 */ x: i16,
        /* offset 0x002 */ y: i16,
        /* offset 0x004 */ z: i16,
        /* offset 0x006 */ w: i16,
    }

    #[derive(Debug, Clone)]
    #[repr(C)]
    struct FSubMeshInfo {
        /* offset 0x000 */ StartVertex: i32,
        /* offset 0x004 */ StartFace: i32,
        /* offset 0x008 */ NumVertices: i32,
        /* offset 0x00c */ NumIndices: i32,
        /* offset 0x010 */ NumFaces: i32,
        /* offset 0x014 */ MaterialType: u32,
        /* offset 0x018 */ NormalZMin: u8,
        /* offset 0x019 */ NormalZMax: u8,
    }

    #[derive(Debug, Clone)]
    #[repr(C)]
    struct FSubVolumeInfo {
        /* offset 0x000 */ MaterialType: u32,
        /* offset 0x004 */ Volume: f32,
        /* offset 0x008 */ ConnectivityIdx: u8,
        /* offset 0x00c */ Center: FVector,
    }

    #[derive(Debug, Clone)]
    #[repr(C)]
    struct FDeepCSGNode {
        /* offset 0x000 */ val: u32,
    }

    #[derive(Debug, Clone)]
    #[repr(C)]
    pub struct FTerrainMeshBox {
        min: FTerrainMeshVertex,
        max: FTerrainMeshVertex,
    }

    #[derive(Debug, Clone)]
    #[repr(C)]
    struct FTerrainMeshVertex {
        x: u16,
        y: u16,
        z: u16,
    }

    #[derive(Debug, Clone)]
    #[repr(C)]
    pub struct FDeepCellStored {
        padding_1: [u8; 0x8],
        sdf_volume: FPFByteData,
        padding_1a: [u8; 0x4],
        solidity: u8,
        tmp_solidity: u8,
        padding_2: [u8; 0xaa],
        mesh_bounding_box: FTerrainMeshBox,
        padding_3: [u8; 0x28],

        pf_solid_volume_from_objects: FPFBitData,
        pf_danger_volume_from_objects: FPFBitData,
        pf_block_volume_from_objects: FPFBitData,

        //padding_4: [u8; 0x34],
        padding_4: [u8; 0xc],
    }
    #[derive(Debug, Clone)]
    #[repr(C)]
    pub struct FDeepCellStoredServer {
        danger_material_volume: FPFBitData,
        prevent_spawn_material_volume: FPFBitData,
        tmp_danger_material_volume: FPFBitData,
        tmp_prevent_spawn_material_volume: FPFBitData,
        padding_1: [u8; 0x190],
    }

    #[derive(Debug, Clone)]
    #[repr(C)]
    pub struct FPFByteData {
        volume1: u32,
        volume2: u32,
        volume3: u32,
    }

    #[derive(Debug, Clone)]
    #[repr(C)]
    pub struct FPFBitData {
        volume1: u32,
        volume2: u32,
        volume3: u32,
    }

    #[derive(Debug, Clone)]
    #[repr(C)]
    pub struct FDeepCellStoredConnectivity {}

    type FnGetCellReal = unsafe extern "system" fn(
        this: NonNull<FCoreCSGWorld>,
        chunkAndOffset: NonNull<FChunkIDAndOffset>,
    ) -> Option<NonNull<FDeepCellStored>>;
    type FnGetCellServerReal = unsafe extern "system" fn(
        this: NonNull<FCoreCSGWorld>,
        chunkAndOffset: NonNull<FChunkIDAndOffset>,
    )
        -> Option<NonNull<FDeepCellStoredServer>>;

    #[derive(Debug)]
    #[repr(u8)]
    enum DeepPathFinderType {
        Walk = 0x0,
        Fly = 0x1,
        MAX = 0x2,
    }

    #[derive(Debug)]
    #[repr(u8)]
    enum DeepPathFinderSize {
        Invalid = 0x0,
        Small = 0x3,
        Medium = 0x2,
        Large = 0x1,
    }
    #[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
    #[repr(u8)]
    enum EPathfinderResult {
        Success = 0x0,
        Failed_StartingPointNotFound = 0x1,
        Failed_EndPointNotFound = 0x2,
        Failed_PointsNotConnected = 0x3,
        Failed_UsedTooManyNodes = 0x4,
        Failed_NotReady = 0x5,
        Failed_UnknownError = 0x6,
    }

    #[derive(Debug)]
    #[repr(u8)]
    enum DeepPathFinderPreference {
        None = 0x0,
        Floor = 0x1,
        Walls = 0x2,
        Ceiling = 0x3,
    }

    #[derive(Debug)]
    #[repr(C)]
    pub struct DeepPathfinder {
        padding: [u8; 0x30],
    }

    type FnGetAllSpawnPointsInSphere = unsafe extern "system" fn(
        this: NonNull<DeepPathfinder>,
        path_method: DeepPathFinderType,
        path_size: DeepPathFinderSize,
        origin: &FVector,
        distance: f32,
        out: &mut TArray<FVector>,
    );

    type FnGetPath = unsafe extern "system" fn(
        this: NonNull<DeepPathfinder>,
        path_method: DeepPathFinderType,
        path_size: DeepPathFinderSize,
        pathPreference: DeepPathFinderPreference,
        start: &FVector,
        end: &FVector,
        path: &mut TArray<FVector>,
        completePath: &mut bool,
    ) -> EPathfinderResult;

    unsafe fn get_cell(
        this: NonNull<FCoreCSGWorld>,
        chunk_and_offset: FChunkIDAndOffset,
    ) -> Option<NonNull<FDeepCellStored>> {
        get_cell_index(this, chunk_and_offset).map(|index| {
            let this = element_ptr!(this => .*);
            element_ptr!(this.cells.start => + (index)).nn().unwrap()
        })
    }

    unsafe fn get_cell_server(
        this: NonNull<FCoreCSGWorld>,
        chunk_and_offset: FChunkIDAndOffset,
    ) -> Option<NonNull<FDeepCellStoredServer>> {
        get_cell_index(this, chunk_and_offset).map(|index| {
            element_ptr!((*this.as_ptr()).cells_pathfinder.start => + (index))
                .nn()
                .unwrap()
        })
    }

    unsafe fn get_cell_index(
        this: NonNull<FCoreCSGWorld>,
        chunk_and_offset: FChunkIDAndOffset,
    ) -> Option<usize> {
        let this = element_ptr!(this => .*);

        let section_index = (((chunk_and_offset.chunk_id.z + 0x40) as usize) << 0x10)
            | (((chunk_and_offset.chunk_id.y + 0x80) as usize) << 0x8)
            | ((chunk_and_offset.chunk_id.x + 0x80) as usize);
        if section_index > 0x4fffff {
            return None;
        }
        /*
        for (i, b) in self.SectionsAllocated.bits.iter().enumerate() {
            if *b != 0 {
                println!("NON ZERO {i:5x} {b:32x}");
            }
        }
        */
        let high_index = section_index >> 6;
        //dbg!(chunk_and_offset);
        //println!(
        //    "0x{:08x} 0x{:08x} {:x} {:x} {} {:b} {:b} {:x}",
        //    section_index,
        //    high_index >> 6,
        //    high_index & 0x3f,
        //    std::mem::offset_of!(FCoreCSGWorld, SectionsAllocated),
        //    section_index & 0x3f,
        //    1 << (high_index & 0x3f),
        //    1 << (section_index & 0x3f),
        //    self.SectionsAllocated.high_bits[high_index >> 6],
        //);
        if 0 == this.sections_allocated.high_bits[high_index >> 6] & (1 << (high_index & 0x3f)) {
            return None;
        }
        if 0 == this.sections_allocated.bits[section_index >> 6] & (1 << (section_index & 0x3f)) {
            return None;
        }
        //println!(
        //    "0x{:08x} {:10b} {:10b}",
        //    section_index >> 6,
        //    self.SectionsAllocated.high_bits[high_index >> 6],
        //    self.SectionsAllocated.bits[section_index >> 6]
        //);
        let cell_offset = element_ptr!(this.section_infos.buffer => + (section_index as usize).*)
            .cell_offset as usize;

        let index = cell_offset
            + (((chunk_and_offset.offset.z as usize * 4) + chunk_and_offset.offset.y as usize) * 4)
            + chunk_and_offset.offset.x as usize;
        Some(index)
    }
}
