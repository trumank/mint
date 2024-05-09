use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::rc::Rc;

use deno_core::error::AnyError;
use deno_core::*;

use element_ptr::element_ptr;

use crate::ue::{self, UClassTrait, UObjectBaseTrait, UObjectTrait, NN as _};

struct UEContext {
    templates: HashMap<NonNull<ue::UClass>, v8::Global<v8::FunctionTemplate>>,
}
impl UEContext {
    fn template<'s>(
        &mut self,
        scope: &mut v8::HandleScope<'s>,
        class: NonNull<ue::UClass>,
    ) -> v8::Global<v8::FunctionTemplate> {
        if let Some(template) = self.templates.get(&class) {
            template.clone()
        } else {
            let template = unsafe {
                println!("NEW TEMPLATE");
                let ustruct = class.ustruct();

                fn callback<'s>(
                    _: &mut v8::HandleScope<'s>,
                    _: v8::FunctionCallbackArguments<'s>,
                    _: v8::ReturnValue<'_>,
                ) {
                    unimplemented!("function callback")
                }

                let func_template = v8::FunctionTemplate::new(scope, callback);

                let name = element_ptr!(class.uobject_base() => .name_private.*).to_string();

                func_template.set_class_name(v8::String::new(scope, &name).unwrap().into());

                let template = func_template.instance_template(scope);
                template.set_internal_field_count(1);

                let mut next_field: Option<NonNull<ue::FField>> =
                    element_ptr!(ustruct => .child_properties.*).nn();
                while let Some(field) = next_field {
                    if element_ptr!(field => .class_private.*.cast_flags.*)
                        .contains(ue::EClassCastFlags::CASTCLASS_FProperty)
                    {
                        let prop: NonNull<ue::FProperty> = field.cast();
                        let prop_data = v8::External::new(scope, prop.as_ptr() as *mut c_void);

                        let name = v8::String::new(
                            scope,
                            &element_ptr!(field => .name_private.*).to_string(),
                        )
                        .unwrap();

                        template.set_accessor_with_configuration(
                            name.into(),
                            v8::AccessorConfiguration::new(get_prop)
                                .setter(set_prop)
                                .data(prop_data.into()),
                        );
                    }
                    next_field = element_ptr!(field => .next.*).nn();
                }

                // TODO is this always a UClass?
                if let Some(parent_class) =
                    element_ptr!(ustruct => .super_struct.* as ue::UClass).nn()
                {
                    let parent = self.template(scope, parent_class);
                    func_template.inherit(v8::Local::new(scope, parent));
                }

                v8::Global::new(scope, func_template)
            };
            self.templates.insert(class, template.clone());
            template
        }
    }
}

fn state_from_scope(scope: &mut v8::HandleScope) -> Rc<RefCell<UEContext>> {
    let context = scope.get_current_context();
    context
        .get_slot::<Rc<RefCell<UEContext>>>(scope)
        .unwrap()
        .clone()
}

fn js_obj<'s>(
    scope: &mut v8::HandleScope<'s>,
    obj: NonNull<ue::UObject>,
) -> v8::Local<'s, v8::Object> {
    unsafe {
        println!("OBJ: {}", obj.uobject_base().get_path_name(None));

        let class = obj.uobject_base().class().unwrap();

        let state = state_from_scope(scope);

        let mut state_mut = state.borrow_mut();
        let template = state_mut.template(scope, class);

        let instance = template
            .open(scope)
            .instance_template(scope)
            .new_instance(scope)
            .unwrap();

        instance.set_internal_field(
            0,
            v8::External::new(scope, obj.as_ptr() as *mut c_void).into(),
        );

        instance
    }
}
fn get_prop(
    scope: &mut v8::HandleScope,
    _property: v8::Local<v8::Name>,
    args: v8::PropertyCallbackArguments,
    mut ret: v8::ReturnValue,
) {
    let this = args.this();
    let prop = unsafe {
        let ext = v8::Local::<v8::External>::cast(args.data());
        (ext.value() as *const ue::FProperty).as_ref().unwrap()
    };

    let prop_class = unsafe { prop.ffield.class_private.as_ref().unwrap() };

    unsafe {
        let external = v8::Local::<v8::External>::cast(this.get_internal_field(scope, 0).unwrap());
        let ptr = external.value().byte_offset(prop.offset_internal as isize) as *mut c_void;

        let flags = prop_class.cast_flags;

        if flags.contains(ue::EClassCastFlags::CASTCLASS_FObjectProperty) {
            if let Some(obj) = NonNull::new(*(ptr as *mut *mut ue::UObject)) {
                ret.set(js_obj(scope, obj).into());
            } else {
                ret.set(v8::null(scope).into());
            }
        } else if flags.contains(ue::EClassCastFlags::CASTCLASS_FBoolProperty) {
            // TODO bitfields
            ret.set(v8::Boolean::new(scope, 0 != *(ptr as *mut u8)).into());
        } else if flags.contains(ue::EClassCastFlags::CASTCLASS_FByteProperty) {
            ret.set(v8::Number::new(scope, *(ptr as *mut i8) as f64).into());
        } else if flags.contains(ue::EClassCastFlags::CASTCLASS_FIntProperty) {
            ret.set(v8::Number::new(scope, *(ptr as *mut i32) as f64).into());
        } else if flags.contains(ue::EClassCastFlags::CASTCLASS_FDoubleProperty) {
            ret.set(v8::Number::new(scope, *(ptr as *mut f64)).into());
        } else if flags.contains(ue::EClassCastFlags::CASTCLASS_FFloatProperty) {
            ret.set(v8::Number::new(scope, *(ptr as *mut f32) as f64).into());
        } else if flags.contains(ue::EClassCastFlags::CASTCLASS_FStrProperty) {
            let s = (ptr as *mut ue::FString).as_ref().unwrap().to_string();
            ret.set(v8::String::new(scope, &s).unwrap().into());
        } else if flags.contains(ue::EClassCastFlags::CASTCLASS_FNameProperty) {
            let s = (ptr as *mut ue::FName).as_ref().unwrap().to_string();
            ret.set(v8::String::new(scope, &s).unwrap().into());
        } else {
            //dbg!(prop);
            //dbg!(prop_class);

            ret.set(
                v8::String::new(scope, &format!("<TODO> {:?}", flags))
                    .unwrap()
                    .into(),
            );
        }
    }
}
fn set_prop(
    scope: &mut v8::HandleScope,
    _property: v8::Local<v8::Name>,
    value: v8::Local<v8::Value>,
    args: v8::PropertyCallbackArguments,
    mut _ret: v8::ReturnValue,
) {
    let this = args.this();
    let prop = unsafe {
        let ext = v8::Local::<v8::External>::cast(args.data());
        (ext.value() as *const ue::FProperty).as_ref().unwrap()
    };
    dbg!(value);

    let prop_class = unsafe { prop.ffield.class_private.as_ref().unwrap() };

    unsafe {
        let external = v8::Local::<v8::External>::cast(this.get_internal_field(scope, 0).unwrap());
        let ptr = external.value().byte_offset(prop.offset_internal as isize) as *mut c_void;

        let flags = prop_class.cast_flags;

        //if flags.contains(ue::EClassCastFlags::CASTCLASS_FObjectProperty) {
        //    if let Some(obj) = NonNull::new(*(ptr as *mut *mut ue::UObject)) {
        //        ret.set(js_obj(scope, obj).into());
        //    } else {
        //        ret.set(v8::null(scope).into());
        //    }
        //} else
        //if flags.contains(ue::EClassCastFlags::CASTCLASS_FBoolProperty) {
        //    // TODO bitfields
        //    ret.set(v8::Boolean::new(scope, 0 != *(ptr as *mut u8)).into());
        //} else
        if flags.contains(ue::EClassCastFlags::CASTCLASS_FByteProperty) {
            if let Some(num) = value.number_value(scope) {
                *(ptr as *mut i8) = num as i8;
            }
        } else if flags.contains(ue::EClassCastFlags::CASTCLASS_FIntProperty) {
            if let Some(num) = value.number_value(scope) {
                *(ptr as *mut i32) = num as i32;
            }
        } else if flags.contains(ue::EClassCastFlags::CASTCLASS_FDoubleProperty) {
            if let Some(num) = value.number_value(scope) {
                *(ptr as *mut f64) = num;
            }
        } else if flags.contains(ue::EClassCastFlags::CASTCLASS_FFloatProperty) {
            if let Some(num) = value.number_value(scope) {
                *(ptr as *mut f32) = num as f32;
            }
        } else if flags.contains(ue::EClassCastFlags::CASTCLASS_FStrProperty) {
            let prop = (ptr as *mut ue::FString).as_mut().unwrap();
            prop.clear();
            prop.extend_from_slice(
                &value
                    .to_rust_string_lossy(scope)
                    .encode_utf16()
                    .chain([0])
                    .collect::<Vec<_>>(),
            );
        //} else if flags.contains(ue::EClassCastFlags::CASTCLASS_FNameProperty) {
        //    let s = (ptr as *mut ue::FName).as_ref().unwrap().to_string();
        //    ret.set(v8::String::new(scope, &s).unwrap().into());
        } else {
            //dbg!(prop);
            //dbg!(prop_class);

            println!("TODO");
            //ret.set(v8::String::new(scope, &format!("<TODO> {:?}", flags)).unwrap().into());
        }
    }
}

#[op2]
fn op_ext_uobject<'s>(scope: &mut v8::HandleScope<'s>, addr: f64) -> v8::Local<'s, v8::Object> {
    let obj: Option<NonNull<ue::UObject>> = NonNull::new((addr as u64) as *mut ue::UObject);
    js_obj(scope, obj.unwrap())
}

#[op2]
pub fn op_ext_callback<'s>(
    scope: &mut v8::HandleScope<'s>,
    #[global] task: v8::Global<v8::Function>,
) {
    let undefined: v8::Local<v8::Value> = v8::undefined(scope).into();

    let tc_scope = &mut v8::TryCatch::new(scope);
    //let js_event_loop_tick_cb = context_state.js_event_loop_tick_cb.borrow();
    let js_event_loop_tick_cb = task.open(tc_scope);

    js_event_loop_tick_cb.call(tc_scope, undefined, &[]);

    //let context_state = JsRealm::state_from_scope(scope);
    //context_state.timers.queue_timer(0, (task, 0)) as _
}

//deno_core::ops!(deno_ops, [op_xyz]);

// Use the ops:
//deno_ops()

pub fn main() {
    // Create the runtime
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();

    // Spawn a future onto the runtime
    rt.block_on(async {
        println!("now running on a worker thread");
        main_async().await.unwrap();
    });
}

async fn main_async() -> Result<(), AnyError> {
    println!("v8 version: {}", deno_core::v8_version());

    const OPS: &[OpDecl] = &[op_ext_uobject(), op_ext_callback()];
    let ext = Extension {
        name: "my_ext",
        ops: std::borrow::Cow::Borrowed(&OPS),
        ..Default::default()
    };

    let mut runtime = deno_core::JsRuntime::new(deno_core::RuntimeOptions {
        extensions: vec![ext],
        module_loader: Some(Rc::new(deno_core::FsModuleLoader)),
        inspector: true,
        ..Default::default()
    });

    runtime.main_context().open(runtime.v8_isolate()).set_slot(
        runtime.v8_isolate(),
        Rc::new(RefCell::new(UEContext {
            templates: Default::default(),
        })),
    );

    //server.register_inspector(
    //    main_module.to_string(),
    //    &mut js_runtime,
    //    options.should_break_on_first_statement || options.should_wait_for_inspector_session,
    //);

    let inspector = deno_inspector::inspector_server::InspectorServer::new(
        std::net::SocketAddr::new(
            std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1)),
            8080,
        ),
        "asdf",
    )
    .unwrap();

    inspector.register_inspector("main".to_string(), &mut runtime, true);
    let op_state = runtime.op_state();
    let inspector = runtime.inspector();
    op_state.borrow_mut().put(inspector);

    runtime
        .inspector()
        .borrow_mut()
        .wait_for_session_and_break_on_next_statement();

    //let file_path = "file:///home/truman/projects/drg-modding/tools/mint/hook/js/main.js";
    let file_path = "./main.js";
    let main_module = deno_core::resolve_path(
        "main.js",
        std::path::Path::new("z:/home/truman/projects/drg-modding/tools/mint/hook/js/"),
    )?;

    let mod_id = runtime.load_main_es_module(&main_module).await?;
    let result = runtime.mod_evaluate(mod_id);
    runtime
        .run_event_loop(PollEventLoopOptions {
            wait_for_inspector: true,
            pump_v8_message_loop: true,
        })
        .await?;
    result.await?;

    //let module = runtime
    //    .load_main_es_module(
    //        &url::Url::parse("https://x.nest.land/ramda@0.27.0/source/index.js").unwrap(),
    //    )
    //    .await
    //    .unwrap();

    //let ret = runtime.mod_evaluate(module).await;

    //println!("ret => {:?}", ret);

    Ok(())
}
