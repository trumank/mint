use std::f32::consts::PI;

use hook_lib::{ue::*, util::NN as _};

#[no_mangle]
pub fn tick(globals: &'static hook_lib::Globals, context: *mut hook_lib::ue::UObject) {
    hook_lib::init_globals(globals);

    unsafe {
        let Some(world) = get_world(context.nn()) else {
            return;
        };

        let batcher = get_batcher(world, 0.).as_mut();

        const RED: FLinearColor = FLinearColor {
            r: 1.,
            g: 0.,
            b: 0.,
            a: 0.,
        };
        const GREEN: FLinearColor = FLinearColor {
            r: 0.,
            g: 1.,
            b: 0.,
            a: 0.,
        };
        const BLUE: FLinearColor = FLinearColor {
            r: 0.,
            g: 0.,
            b: 1.,
            a: 0.,
        };

        let n = 20;
        let r = 1000.;
        for i in 0..n {
            let a = (i as f32 / n as f32) * PI * 2.;
            #[rustfmt::skip]
                let shape = DebugBox {
                    center: FVector { x: r * a.sin(), y: r * a.cos(), z: 0., },
                    extent: FVector { x: 100., y: 100., z: 800., },
                    color: FLinearColor { r: 1., g: 1., b: 0., a: 0. },
                    rotation: FRotator { pitch: 0., yaw: 0., roll: 0. },
                    duration: 0.,
                    thickness: 10.,
                };
            //draw_debug_box(batcher, &shape);
        }
        #[rustfmt::skip]
            let shape = DebugSphere {
                center: FVector { x: 100., y: 100., z: 100., },
                radius: 100.,
                num_segments: 10,
                color: GREEN,
                duration: 0.,
                thickness: 1.,
            };
        //draw_debug_sphere(batcher, &shape);

        //let obj = &*context;
        //println!(
        //    "ayy?? {:X?}",
        //    context,
        //);
    }
}
