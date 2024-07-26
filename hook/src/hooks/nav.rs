use element_ptr::element_ptr;
use hook_lib::globals;
use std::ffi::c_void;
use std::ptr::NonNull;

use hook_lib::ue::get_world;
use hook_lib::ue::FBatchedLine;
use hook_lib::ue::FBatchedPoint;
use hook_lib::ue::FLinearColor;
use hook_lib::ue::FVector;
use hook_lib::ue::TArray;
use hook_lib::ue::UObject;
use hook_lib::ue::UWorld;
use hook_lib::util::NN as _;

use super::debug_drawing::draw_box;
use super::debug_drawing::draw_lines;
use super::debug_drawing::draw_points;
use super::debug_drawing::get_batcher;
use super::ue;
use super::ExecFn;

pub fn kismet_hooks() -> &'static [(&'static str, ExecFn)] {
    &[
        (
            "/Game/_AssemblyStorm/TestMod/DebugStuff.DebugStuff_C:ReceiveTick",
            exec_tick as ExecFn,
        ),
        (
            "/Game/_AssemblyStorm/TestMod/MintDebugStuff/InitCave.InitCave_C:PathTo",
            exec_path_to as ExecFn,
        ),
        (
            "/Game/_AssemblyStorm/TestMod/MintDebugStuff/InitCave.InitCave_C:SpawnPoints",
            exec_spawn_points as ExecFn,
        ),
        (
            "/Game/_mint/BPL_CSG.BPL_CSG_C:Get Procedural Mesh Vertices",
            exec_get_mesh_vertices as ExecFn,
        ),
        (
            "/Game/_mint/BPL_CSG.BPL_CSG_C:Get Procedural Mesh Triangles",
            exec_get_mesh_triangles as ExecFn,
        ),
    ]
}

#[repr(C)]
struct AFSDGameState {
    object: ue::UObject,

    padding: [u8; 0x3f8],

    csg_world: *const ADeepCSGWorld,
    // TODO
}

unsafe extern "system" fn exec_tick(
    context: *mut ue::UObject,
    stack: *mut ue::kismet::FFrame,
    _result: *mut c_void,
) {
    let stack = stack.as_mut().unwrap();

    let _delta_seconds: f32 = stack.arg();

    if let Some(world) = get_world(context.nn()) {
        render(world);
    }

    if !stack.code.is_null() {
        stack.code = stack.code.add(1);
    }
}

unsafe extern "system" fn exec_path_to(
    context: *mut ue::UObject,
    stack: *mut ue::kismet::FFrame,
    _result: *mut c_void,
) {
    let stack = stack.as_mut().unwrap();

    let dest: FVector = stack.arg();
    let size: DeepPathFinderSize = stack.arg();
    let type_: DeepPathFinderType = stack.arg();
    let pref: DeepPathFinderPreference = stack.arg();
    let follow_player: bool = stack.arg();

    if let Some(world) = get_world(context.nn()) {
        path_to(world, follow_player.then_some(dest), size, type_, pref);
    }

    if !stack.code.is_null() {
        stack.code = stack.code.add(1);
    }
}

unsafe extern "system" fn exec_spawn_points(
    context: *mut ue::UObject,
    stack: *mut ue::kismet::FFrame,
    _result: *mut c_void,
) {
    let stack = stack.as_mut().unwrap();

    let dest: FVector = stack.arg();
    let radius: f32 = stack.arg();
    let size: DeepPathFinderSize = stack.arg();
    let type_: DeepPathFinderType = stack.arg();

    if let Some(world) = get_world(context.nn()) {
        spawn_points(world, &dest, radius, size, type_);
    }

    if !stack.code.is_null() {
        stack.code = stack.code.add(1);
    }
}

#[derive(Debug, Clone)]
#[repr(C)]
pub struct UDeepProceduralMeshComponent {
    padding: [u8; 0x488],
    chunk_id: FChunkId,
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
        pub get_triangle_mesh_flags: unsafe extern "system" fn(this: NonNull<PxTriangleMesh>, &mut u8) -> u8,
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

unsafe extern "system" fn exec_get_mesh_triangles(
    _context: *mut ue::UObject,
    stack: *mut ue::kismet::FFrame,
    _result: *mut c_void,
) {
    let stack = stack.as_mut().unwrap();

    let mesh: Option<NonNull<UDeepProceduralMeshComponent>> = stack.arg();
    let _world_context: Option<NonNull<UObject>> = stack.arg();

    #[derive(Debug, Clone, Copy)]
    #[repr(C)]
    struct Tri<T> {
        a: T,
        b: T,
        c: T,
    }
    impl From<Tri<u16>> for Tri<u32> {
        fn from(value: Tri<u16>) -> Self {
            Self {
                a: value.a as u32,
                b: value.b as u32,
                c: value.c as u32,
            }
        }
    }

    drop(stack.arg::<TArray<Tri<u32>>>());
    let ret: &mut TArray<Tri<u32>> = &mut *(stack.most_recent_property_address as *mut _);
    *ret = TArray::new();

    ret.clear();

    if let Some(mesh) = mesh {
        if let Some(triangle_mesh) = element_ptr!(mesh => .triangle_mesh.*).nn() {
            let vtable = element_ptr!(triangle_mesh => .vftable.*);
            let num = element_ptr!(vtable => .get_nb_triangles.*)(triangle_mesh);
            let ptr = element_ptr!(vtable => .get_triangles.*)(triangle_mesh);
            let mut flags = 0;
            let _ = element_ptr!(vtable => .get_triangle_mesh_flags.*)(triangle_mesh, &mut flags);
            if flags & 2 != 0 {
                for tri in std::slice::from_raw_parts(ptr as *const Tri<u16>, num as usize) {
                    ret.push((*tri).into());
                }
            } else {
                ret.extend_from_slice(std::slice::from_raw_parts(
                    ptr as *const Tri<u32>,
                    num as usize,
                ));
            }
        }
    }

    if !stack.code.is_null() {
        stack.code = stack.code.add(1);
    }
}

unsafe fn get_path(
    pathfinder: NonNull<DeepPathfinder>,
    start: &FVector,
    end: &FVector,
    size: DeepPathFinderSize,
    type_: DeepPathFinderType,
    pref: DeepPathFinderPreference,
) -> Option<Vec<FVector>> {
    let Ok(get_path) = globals()
        .resolution
        .get_path
        .as_ref()
        .map(|r| std::mem::transmute::<usize, FnGetPath>(r.0 as usize))
    else {
        return None;
    };

    let mut path = vec![];

    let mut start = *start;

    loop {
        let mut tmp = TArray::default();
        let mut complete = false;
        let res = get_path(
            pathfinder,
            type_,
            size,
            pref,
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
    dest: Option<FVector>,
    csg_world: NonNull<ADeepCSGWorld>,
    size: DeepPathFinderSize,
    type_: DeepPathFinderType,
    pref: DeepPathFinderPreference,
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
                &dest.unwrap_or_else(|| FVector::new(-1000.0, i as f32 * 100.0, 0.)),
                size,
                type_,
                pref,
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
    for x in 0..0 {
        // TODO
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

pub unsafe fn path_to(
    world: NonNull<UWorld>,
    dest: Option<FVector>,
    size: DeepPathFinderSize,
    type_: DeepPathFinderType,
    pref: DeepPathFinderPreference,
) {
    let batcher = get_batcher(world, 0.).as_mut();

    let mut lines = vec![];
    let mut points = vec![];

    let game_state = element_ptr!(world => .game_state.* as AFSDGameState);
    let csg_world = element_ptr!(game_state => .csg_world.*).nn();
    if let Some(csg_world) = csg_world {
        path_stuff(&mut lines, &mut points, dest, csg_world, size, type_, pref);
    }

    draw_lines(batcher, &lines);
    draw_points(batcher, &points);
}

pub unsafe fn spawn_points(
    world: NonNull<UWorld>,
    dest: &FVector,
    radius: f32,
    size: DeepPathFinderSize,
    type_: DeepPathFinderType,
) {
    let Ok(get_all_spawn_points) = globals()
        .resolution
        .get_all_spawn_points_in_sphere
        .as_ref()
        .map(|r| std::mem::transmute::<usize, FnGetAllSpawnPointsInSphere>(r.0 as usize))
    else {
        return;
    };

    let batcher = get_batcher(world, 0.).as_mut();

    let mut lines = vec![];
    let mut points = vec![];

    let game_state = element_ptr!(world => .game_state.* as AFSDGameState);
    let csg_world = element_ptr!(game_state => .csg_world.*).nn();
    if let Some(csg_world) = csg_world {
        if let Some(pathfinder) = element_ptr!(csg_world => .pathfinder.*).nn() {
            let mut spawn_points = TArray::default();
            get_all_spawn_points(pathfinder, type_, size, dest, radius, &mut spawn_points);

            for point in spawn_points.as_slice() {
                draw_box(
                    &mut lines,
                    *point,
                    FVector::new(30., 30., 30.),
                    FLinearColor::new(0., 0., 1., 1.),
                );
            }
        }
    }

    draw_lines(batcher, &lines);
    draw_points(batcher, &points);
}

pub unsafe fn render(world: NonNull<UWorld>) {
    let batcher = get_batcher(world, 0.).as_mut();

    let mut lines = vec![];
    let mut points = vec![];

    //let get_all_spawn_points: FnGetAllSpawnPointsInSphere = std::mem::transmute(0x143dc28a0 as usize);

    let game_state = element_ptr!(world => .game_state.* as AFSDGameState);
    let csg_world = element_ptr!(game_state => .csg_world.*).nn();
    if let Some(csg_world) = csg_world {
        //println!("csg_world {csg_world:?}");

        //path_stuff(&mut lines, &mut points, csg_world);
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
    ConnectivitySidePoints: DeepArrayPool<FTerrainSideConnectivity, FDeepCellStoredConnectivity>,
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

    const _: [u8; 0x420] = [0; std::mem::offset_of!(AFSDGameState, csg_world)];
    //const _: [u8; 0x128] = [0; std::mem::size_of::<FDeepCellStored>()];
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
) -> Option<NonNull<FDeepCellStoredServer>>;

#[derive(Debug, Default, Clone, Copy)]
#[repr(u8)]
pub enum DeepPathFinderType {
    #[default]
    Walk = 0x0,
    Fly = 0x1,
    MAX = 0x2,
}

#[derive(Debug, Default, Clone, Copy)]
#[repr(u8)]
pub enum DeepPathFinderSize {
    Invalid = 0x0,
    #[default]
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

#[derive(Debug, Default, Clone, Copy)]
#[repr(u8)]
pub enum DeepPathFinderPreference {
    #[default]
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
