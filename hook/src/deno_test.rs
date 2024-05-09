//use deno_runtime::deno_core;
//use deno_runtime::deno_core::*;

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::rc::Rc;

use deno_core::error::AnyError;
use deno_core::*;

use element_ptr::element_ptr;

use crate::ue::{self, UClassTrait, UObjectBaseTrait, UObjectTrait, NN as _};

struct FVector {
    x: f32,
    y: f32,
    z: f32,
}

struct ExternalObject(RefCell<u32>);
struct ExternalVector(RefCell<Vec<u32>>);

external!(ExternalObject, "test external object");
external!(ExternalVector, "test external object");

#[op2(fast)]
fn op_xyz() {
    println!("HUH");
}

#[op2(fast)]
fn op_ext_obj() -> *const std::ffi::c_void {
    // This operation is safe because we know
    ExternalPointer::new(ExternalObject(RefCell::new(42))).into_raw()
}
#[op2(fast)]
fn op_ext_obj_process(ptr: *const std::ffi::c_void) {
    let ptr = ExternalPointer::<ExternalObject>::from_raw(ptr);
    *(unsafe { ptr.unsafely_deref() }.0.borrow_mut()) += 1;
}
#[op2(fast)]
fn op_ext_obj_take(ptr: *const std::ffi::c_void) -> u32 {
    let ptr = ExternalPointer::<ExternalObject>::from_raw(ptr);
    *unsafe { ptr.unsafely_take() }.0.borrow()
}

#[op2(fast)]
fn op_ext_vec() -> *const std::ffi::c_void {
    // This operation is safe because we know
    ExternalPointer::new(ExternalVector(RefCell::new(vec![]))).into_raw()
}
#[op2(fast)]
fn op_ext_vec_process(ptr: *const std::ffi::c_void) {
    let ptr = ExternalPointer::<ExternalVector>::from_raw(ptr);
    let mut buf = unsafe { ptr.unsafely_deref() }.0.borrow_mut();
    let next = buf.len() as u32;
    buf.push(next);
}
#[op2]
#[serde]
fn op_ext_vec_take(ptr: *const std::ffi::c_void) -> Vec<u32> {
    let ptr = ExternalPointer::<ExternalVector>::from_raw(ptr);
    unsafe { ptr.unsafely_take() }.0.borrow().to_vec()
}

static mut DATA: FVector = FVector {
    x: 1.,
    y: 2.,
    z: 4.,
};

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

                        let name = element_ptr!(field => .name_private.*);
                        let string = name.to_string();

                        let name = v8::String::new(scope, &string).unwrap();

                        let prop_data = v8::External::new(scope, prop.as_ptr() as *mut c_void);

                        template.set_accessor_with_configuration(
                            name.into(),
                            v8::AccessorConfiguration::new(get_x)
                                .setter(set_x)
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
fn get_x(
    scope: &mut v8::HandleScope,
    property: v8::Local<v8::Name>,
    args: v8::PropertyCallbackArguments,
    mut ret: v8::ReturnValue,
) {
    let this = args.this();
    let prop = unsafe {
        let ext = v8::Local::<v8::External>::cast(args.data());
        (ext.value() as *const ue::FProperty).as_ref().unwrap()
    };

    //dbg!(prop);

    let prop_class = unsafe { prop.ffield.class_private.as_ref().unwrap() };

    //dbg!(prop_class);

    let external = this.get_internal_field(scope, 0).unwrap();
    let external = unsafe { v8::Local::<v8::External>::cast(external) };
    let obj = unsafe {
        NonNull::new(
            *(external.value().byte_offset(prop.offset_internal as isize)
                as *const *mut ue::UObject),
        )
    };

    if prop_class
        .cast_flags
        .contains(ue::EClassCastFlags::CASTCLASS_FObjectProperty)
    {
        if let Some(obj) = obj {
            ret.set(js_obj(scope, obj).into());
        } else {
            ret.set(v8::null(scope).into());
        }
    } else {
        ret.set(v8::String::new(scope, "<TODO>").unwrap().into());
    }
}
fn set_x(
    scope: &mut v8::HandleScope,
    property: v8::Local<v8::Name>,
    value: v8::Local<v8::Value>,
    args: v8::PropertyCallbackArguments,
    mut ret: v8::ReturnValue,
) {
    if value.is_number() {
        if let Some(num) = value.to_number(scope) {
            let this = args.this();

            println!("num fields {}", this.internal_field_count());

            let external = this.get_internal_field(scope, 0).unwrap();
            let external = unsafe { v8::Local::<v8::External>::cast(external) };
            let vec = unsafe { (external.value() as *mut FVector).as_mut().unwrap() };
            vec.x = num.value() as f32;
        }
    }

    //ret.set(v8::Number::new(scope, vec.x.into()).into());
}

#[op2]
fn op_ext_uobject<'s>(scope: &mut v8::HandleScope<'s>, addr: f64) -> v8::Local<'s, v8::Object> {
    let context = state_from_scope(scope);

    let obj: Option<NonNull<ue::UObject>> = NonNull::new((addr as u64) as *mut ue::UObject);
    return js_obj(scope, obj.unwrap());

    //context.templates.

    //let obj = v8::Object::new(scope);
    //let key = v8::String::new(scope, "key").unwrap();
    //let value = v8::String::new(scope, "value").unwrap();
    //obj.set(scope, key.into(), value.into());

    //let key = v8::String::new(scope, "key2").unwrap();
    //let value = v8::External::new(
    //    scope,
    //    ExternalPointer::new(ExternalVector(RefCell::new(vec![1])))
    //        .into_raw()
    //        .cast_mut(),
    //);
    //obj.set(scope, key.into(), value.into());

    let x_name = v8::String::new(scope, "x").unwrap();

    let vector_template = v8::ObjectTemplate::new(scope);
    vector_template.set_internal_field_count(1);
    vector_template.set_accessor_with_setter(x_name.into(), get_x, set_x);

    let instance = vector_template.new_instance(scope).unwrap();
    instance.set_internal_field(
        0,
        v8::External::new(scope, unsafe {
            std::ptr::addr_of_mut!(DATA) as *mut std::ffi::c_void
        })
        .into(),
    );

    println!("num fields {}", instance.internal_field_count());

    //instance.set_accessor(scope, v8::String::new(scope, "x").unwrap().into(), get_x);
    //instance.set_accessor(scope, v8::String::new(scope, "x").unwrap().into(), get_x);
    //vector_template.set
    //file_template->Set(isolate, "read", FunctionTemplate::New(isolate, Shell::ReadFile));

    instance
    //obj
}

#[allow(clippy::needless_pass_by_value)] // this function should follow the callback type
fn request_prop_handler(
    scope: &mut v8::HandleScope,
    key: v8::Local<v8::Name>,
    args: v8::PropertyCallbackArguments,
    mut rv: v8::ReturnValue,
) {
    //let this = args.this();
    //let external = Self::unwrap_request(scope, this);

    //assert!(
    //  !external.is_null(),
    //  "the pointer to Box<dyn HttpRequest> should not be null"
    //);

    //let request = unsafe { &mut *external };

    //let key = key.to_string(scope).unwrap().to_rust_string_lossy(scope);

    //let value = match &*key {
    //  "path" => request.path(),
    //  "userAgent" => request.user_agent(),
    //  "referrer" => request.referrer(),
    //  "host" => request.host(),
    //  _ => {
    //    return;
    //  }
    //};

    //rv.set(v8::String::new(scope, value).unwrap().into());
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

    const OPS: &[OpDecl] = &[
        op_xyz(),
        op_ext_obj(),
        op_ext_obj_take(),
        op_ext_obj_process(),
        op_ext_vec(),
        op_ext_vec_take(),
        op_ext_vec_process(),
        op_ext_uobject(),
        op_ext_callback(),
    ];
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

    let ctx = runtime.main_context().clone();
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
