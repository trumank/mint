use std::ffi::c_void;
use std::ptr::NonNull;

use element_ptr::element_ptr;
use na::{Matrix, Matrix4, Point3, Vector3};
use nalgebra as na;

use crate::hooks::ExecFn;
use crate::ue::{self, FLinearColor, FRotator, FVector, TArray};

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
    loop {
        let Some(outer) = ctx else {
            break;
        };
        let class = element_ptr!(outer => .uobject_base_utility.uobject_base.class_private.*).nn();
        if let Some(class) = class {
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

        let mut lines = Vec::new();
        lines.reserve(num_segments_y as usize * segments as usize * 2);

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

    let angle1 = angle_height.clamp(std::f32::EPSILON, std::f32::consts::PI - std::f32::EPSILON);
    let angle2 = angle_width.clamp(std::f32::EPSILON, std::f32::consts::PI - std::f32::EPSILON);

    let sin_x_2 = (0.5 * angle1).sin();
    let sin_y_2 = (0.5 * angle2).sin();

    let sin_sq_x_2 = sin_x_2 * sin_x_2;
    let sin_sq_y_2 = sin_y_2 * sin_y_2;

    let mut cone_verts = Vec::with_capacity(num_sides as usize);

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
    for i in 0..num_sides {
        current_point = cone_to_world.transform_point(&cone_verts[i].into()).coords;
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

unsafe extern "system" fn exec_draw_debug_box(
    _context: *mut ue::UObject,
    stack: *mut ue::kismet::FFrame,
    _result: *mut c_void,
) {
    let stack = stack.as_mut().unwrap();

    let world_context: Option<NonNull<ue::UObject>> = stack.arg();
    let center: FVector = stack.arg();
    let extent: FVector = stack.arg();
    let color: FLinearColor = stack.arg();
    let rotation: FRotator = stack.arg();
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

        draw_lines(batcher, &lines);
    }

    if !stack.code.is_null() {
        stack.code = stack.code.add(1);
    }
}
