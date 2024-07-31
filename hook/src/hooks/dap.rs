use std::collections::{BTreeMap, HashMap};
use std::ffi::c_void;
use std::io::{BufReader, BufWriter};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard};

use anyhow::{Context as _, Result};
use byteorder::{ReadBytesExt as _, LE};
use dap::server::ServerOutput;
use kismet::FFrame;
use thiserror::Error;

use dap::prelude::responses::*;
use dap::prelude::*;

use crate::globals;
use crate::ue::*;

use super::ExecFn;

#[derive(Error, Debug)]
enum MyAdapterError {
    #[error("Unhandled command")]
    UnhandledCommandError(String),

    #[error("Missing command")]
    MissingCommandError,
}

type DynResult<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub unsafe fn init() -> Result<()> {
    if let Ok(debug) = &globals().resolution.debug {
        tracing::info!("hooking GNatives");
        hook_gnatives((debug.gnatives.0 as *mut NativesArray).as_mut().unwrap());

        std::thread::spawn(|| {
            listen().unwrap();
        });
    }
    Ok(())
}

pub fn listen() -> DynResult<()> {
    let listener = TcpListener::bind("127.0.0.1:7778")?;

    // accept connections and process them serially
    for stream in listener.incoming() {
        let s = stream?;
        let i = s.try_clone()?;
        //stream?.split
        handle(Box::new(i), Box::new(s))?;
    }
    Ok(())
}

fn handle(input: Box<dyn std::io::Read>, output: Box<dyn std::io::Write + Send>) -> DynResult<()> {
    tracing::info!("handling");
    //let output = BufWriter::new(std::io::stdout());
    //let f = File::open("testinput.txt")?;
    //let input = BufReader::new(f);
    let mut server = Server::new(BufReader::new(input), BufWriter::new(output));

    *DAP_OUTPUT.lock().unwrap() = Some(server.output.clone());

    loop {
        let req = match server.poll_request()? {
            Some(req) => req,
            None => return Err(Box::new(MyAdapterError::MissingCommandError)),
        };
        tracing::info!("{req:#?}");
        match req.command {
            Command::Initialize(_) => {
                let rsp = req.success(ResponseBody::Initialize(types::Capabilities {
                    supports_configuration_done_request: Some(true),
                    supports_function_breakpoints: Some(true),
                    supports_conditional_breakpoints: Some(true),
                    supports_hit_conditional_breakpoints: Some(true),
                    supports_evaluate_for_hovers: Some(false), // TODO
                    //exception_breakpoint_filters: Some(true),
                    supports_step_back: Some(true),
                    supports_set_variable: Some(true),
                    supports_restart_frame: Some(true),
                    supports_goto_targets_request: Some(true),
                    supports_step_in_targets_request: Some(true),
                    supports_completions_request: Some(true),
                    //completion_trigger_characters: Some(true),
                    supports_modules_request: Some(true),
                    //additional_module_columns: Some(true),
                    //supported_checksum_algorithms: Some(true),
                    supports_restart_request: Some(true),
                    supports_exception_options: Some(true),
                    supports_value_formatting_options: Some(true),
                    supports_exception_info_request: Some(true),
                    support_terminate_debuggee: Some(true),
                    support_suspend_debuggee: Some(true),
                    supports_delayed_stack_trace_loading: Some(true),
                    supports_loaded_sources_request: Some(true),
                    supports_log_points: Some(true),
                    supports_terminate_threads_request: Some(true),
                    supports_set_expression: Some(true),
                    supports_terminate_request: Some(true),
                    supports_data_breakpoints: Some(true),
                    supports_read_memory_request: Some(true),
                    supports_write_memory_request: Some(true),
                    supports_disassemble_request: Some(true),
                    supports_cancel_request: Some(true),
                    supports_breakpoint_locations_request: Some(true),
                    supports_clipboard_context: Some(true),
                    supports_stepping_granularity: Some(true),
                    supports_instruction_breakpoints: Some(true),
                    supports_exception_filter_options: Some(true),
                    supports_single_thread_execution_requests: Some(true),
                    ..Default::default()
                }));

                // When you call respond, send_event etc. the message will be wrapped
                // in a base message with a appropriate seq number, so you don't have to keep track of that yourself
                tracing::info!("rsp = {:#?}", rsp);
                server.respond(rsp)?;

                server.send_event(Event::Initialized)?;

                REQUEST_PAUSE.store(true, Ordering::SeqCst);
            }
            Command::Attach(_) => {
                server.respond(req.success(ResponseBody::Attach))?;
            }
            Command::SetBreakpoints(_) => {
                server.respond(req.success(ResponseBody::SetBreakpoints(
                    SetBreakpointsResponse {
                        breakpoints: vec![],
                    },
                )))?;
            }
            Command::SetFunctionBreakpoints(_) => {
                server.respond(req.success(ResponseBody::SetFunctionBreakpoints(
                    SetFunctionBreakpointsResponse {
                        breakpoints: vec![],
                    },
                )))?;
            }
            Command::SetInstructionBreakpoints(_) => {
                server.respond(req.success(ResponseBody::SetInstructionBreakpoints(
                    SetInstructionBreakpointsResponse {
                        breakpoints: vec![],
                    },
                )))?;
            }
            Command::SetDataBreakpoints(_) => {
                server.respond(req.success(ResponseBody::SetDataBreakpoints(
                    SetDataBreakpointsResponse {
                        breakpoints: vec![],
                    },
                )))?;
            }
            Command::BreakpointLocations(_) => {
                server.respond(req.success(ResponseBody::BreakpointLocations(
                    BreakpointLocationsResponse {
                        breakpoints: vec![],
                    },
                )))?;
            }
            Command::ConfigurationDone => {
                server.respond(req.success(ResponseBody::ConfigurationDone))?;
            }
            Command::Threads => {
                server.respond(req.success(ResponseBody::Threads(ThreadsResponse {
                    threads: vec![types::Thread {
                        id: 1,
                        name: "Main".into(),
                    }],
                })))?;
            }
            Command::StackTrace(ref cmd) => {
                if cmd.thread_id == 1 {
                    let res = if let Some(state) = PAUSE_STATE.lock().unwrap().as_ref() {
                        let mut frames = vec![];

                        let mut frame = state.frame.cast_const();

                        let mut i = 0;
                        while let Some(f) = unsafe { frame.as_ref() } {
                            let ufunc = unsafe { &*f.node };

                            if i >= cmd.start_frame.unwrap_or(0)
                                && frames.len() < cmd.levels.unwrap_or(i64::MAX) as usize
                            {
                                let path = ufunc
                                    .ustruct
                                    .ufield
                                    .uobject
                                    .uobject_base_utility
                                    .uobject_base
                                    .get_path_name(None);

                                let name = ufunc
                                    .ustruct
                                    .ufield
                                    .uobject
                                    .uobject_base_utility
                                    .uobject_base
                                    .name_private
                                    .to_string();

                                let mut lock = SOURCES.lock().unwrap();

                                let src = get_source(&mut lock, ufunc);

                                let line = src
                                    .1
                                    .index_to_line
                                    .get(&state.index)
                                    .map(|l| *l as i64)
                                    .unwrap_or(1);

                                tracing::info!("{:#?}", src.1.index_to_line);
                                tracing::info!("index = {} line = {line}", state.index);

                                frames.push(types::StackFrame {
                                    id: frame as i64,
                                    name: name.clone(),
                                    source: Some(types::Source {
                                        name: Some(name),
                                        path: None, //Some(path),
                                        source_reference: Some(src.0),
                                        presentation_hint: None,
                                        origin: Some("internal module".into()),
                                        sources: None,
                                        adapter_data: None,
                                        checksums: None,
                                    }),
                                    line,
                                    column: 1,
                                    end_line: None,
                                    end_column: None,
                                    can_restart: None,
                                    instruction_pointer_reference: None,
                                    module_id: None,
                                    presentation_hint: None,
                                });
                            }

                            frame = f.previous_frame;

                            i += 1;
                        }

                        StackTraceResponse {
                            stack_frames: frames,
                            total_frames: Some(i),
                        }
                    } else {
                        StackTraceResponse {
                            stack_frames: vec![],
                            total_frames: Some(0),
                        }
                    };

                    server.respond(req.success(ResponseBody::StackTrace(res)))?;
                } else {
                    todo!("other threads ({})", cmd.thread_id);
                }
            }
            Command::Source(ref cmd) => {
                let id = cmd.source.as_ref().unwrap().source_reference.unwrap();
                let sources = SOURCES.lock().unwrap();
                let content = get_source_from_id(&sources, id).unwrap().content.clone();

                server.respond(req.success(ResponseBody::Source(responses::SourceResponse {
                    content,
                    mime_type: None,
                })))?;
            }
            Command::Scopes(ref cmd) => {
                // TODO
                let frame = unsafe { &*(cmd.frame_id as *const FFrame) };
                let func = unsafe { &*frame.node };

                let id = cmd.frame_id;

                let mut count = 0;
                for (s, p) in func.ustruct.properties() {
                    count += 1;
                    tracing::info!(
                        "{}: {}.{}",
                        p.offset_internal,
                        s.ufield
                            .uobject
                            .uobject_base_utility
                            .uobject_base
                            .name_private,
                        p.ffield.name_private
                    );
                }
                server.respond(req.success(ResponseBody::Scopes(responses::ScopesResponse {
                    scopes: vec![types::Scope {
                        name: "Locals".into(),
                        presentation_hint: Some(types::ScopePresentationhint::Locals),
                        variables_reference: id,
                        named_variables: Some(count),
                        indexed_variables: None,
                        expensive: false,
                        source: None,
                        line: None,
                        column: None,
                        end_line: None,
                        end_column: None,
                    }],
                })))?;
            }
            Command::Variables(ref cmd) => {
                let frame = unsafe { &*(cmd.variables_reference as *const FFrame) };
                let func = unsafe { &*frame.node };

                let mut variables = vec![];
                for (s, p) in func.ustruct.properties() {
                    variables.push(types::Variable {
                        name: p.ffield.name_private.to_string(),
                        value: get_prop_as_string(frame.locals, p),
                        type_field: Some("TODO".into()),
                        presentation_hint: None,
                        evaluate_name: None,
                        variables_reference: 0,
                        named_variables: None,
                        indexed_variables: None,
                        memory_reference: None,
                    })
                    //tracing::info!(
                    //    "{}: {}.{}",
                    //    p.offset_internal,
                    //    s.ufield
                    //        .uobject
                    //        .uobject_base_utility
                    //        .uobject_base
                    //        .name_private,
                    //    p.ffield.name_private
                    //);
                }

                server.respond(req.success(ResponseBody::Variables(
                    responses::VariablesResponse { variables },
                )))?;
            }
            Command::Continue(_) => {
                PAUSE_STATE.lock().unwrap().take();
                server.respond(req.success(ResponseBody::Continue(
                    responses::ContinueResponse {
                        all_threads_continued: None,
                    },
                )))?;
            }
            Command::StepIn(_) => {
                REQUEST_PAUSE.store(true, Ordering::SeqCst);
                PAUSE_STATE.lock().unwrap().take();
                server.respond(req.success(ResponseBody::StepIn))?;
            }
            Command::StepOut(_) => {
                REQUEST_PAUSE.store(true, Ordering::SeqCst);
                PAUSE_STATE.lock().unwrap().take();
                server.respond(req.success(ResponseBody::StepOut))?;
            }
            Command::StepBack(_) => {
                REQUEST_PAUSE.store(true, Ordering::SeqCst);
                PAUSE_STATE.lock().unwrap().take();
                server.respond(req.success(ResponseBody::StepBack))?;
            }
            Command::Pause(ref cmd) => {
                if cmd.thread_id == 1 {
                    REQUEST_PAUSE.store(true, Ordering::SeqCst);
                    PAUSE_STATE.lock().unwrap().take();
                    server.respond(req.success(ResponseBody::Pause))?;
                } else {
                    unimplemented!();
                }
            }
            Command::Disconnect(_) => {
                server.respond(req.success(ResponseBody::Disconnect))?;
                break;
            }
            cmd => {
                return Err(Box::new(MyAdapterError::UnhandledCommandError(format!(
                    "{cmd:#?}"
                ))));
            }
        }
    }
    Ok(())
}

fn get_prop_as_string(obj: *const c_void, prop: &FProperty) -> String {
    unsafe {
        let prop_class = &*prop.ffield.class_private;

        let ptr = obj.byte_offset(prop.offset_internal as isize);

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
        if flags.contains(EClassCastFlags::CASTCLASS_FByteProperty) {
            (*(ptr as *const i8)).to_string()
        } else if flags.contains(EClassCastFlags::CASTCLASS_FBoolProperty) {
            // TODO bitfields
            (*(ptr as *const bool)).to_string()
        } else if flags.contains(EClassCastFlags::CASTCLASS_FIntProperty) {
            (*(ptr as *const i32)).to_string()
        } else if flags.contains(EClassCastFlags::CASTCLASS_FDoubleProperty) {
            (*(ptr as *const f64)).to_string()
        } else if flags.contains(EClassCastFlags::CASTCLASS_FFloatProperty) {
            (*(ptr as *const f32)).to_string()
        } else if flags.contains(EClassCastFlags::CASTCLASS_FStrProperty) {
            (*(ptr as *const FString)).to_string()
        } else if flags.contains(EClassCastFlags::CASTCLASS_FNameProperty) {
            (*(ptr as *const FName)).to_string()
        } else if flags.contains(EClassCastFlags::CASTCLASS_FObjectProperty) {
            (*(ptr as *const *const UObject)).as_ref().map_or_else(
                || "null".into(),
                |o| o.uobject_base_utility.uobject_base.get_path_name(None),
            )
        } else {
            //dbg!(prop);
            //dbg!(prop_class);

            format!("<TODO> {:?}", flags)
        }

        //if flags.contains(ue::EClassCastFlags::CASTCLASS_FObjectProperty) {
        //    if let Some(obj) = NonNull::new(*(ptr as *mut *mut ue::UObject)) {
        //        js_obj(scope, obj).into()
        //    } else {
        //        v8::null(scope).into()
        //    }
        //} else if flags.contains(ue::EClassCastFlags::CASTCLASS_FBoolProperty) {
        //    // TODO bitfields
        //    v8::Boolean::new(scope, 0 != *(ptr as *mut u8)).into()
        //} else if flags.contains(ue::EClassCastFlags::CASTCLASS_FByteProperty) {
        //    v8::Number::new(scope, *(ptr as *mut i8) as f64).into()
        //} else if flags.contains(ue::EClassCastFlags::CASTCLASS_FIntProperty) {
        //    v8::Number::new(scope, *(ptr as *mut i32) as f64).into()
        //} else if flags.contains(ue::EClassCastFlags::CASTCLASS_FDoubleProperty) {
        //    v8::Number::new(scope, *(ptr as *mut f64)).into()
        //} else if flags.contains(ue::EClassCastFlags::CASTCLASS_FFloatProperty) {
        //    v8::Number::new(scope, *(ptr as *mut f32) as f64).into()
        //} else if flags.contains(ue::EClassCastFlags::CASTCLASS_FStrProperty) {
        //    let s = (ptr as *mut ue::FString).as_ref().unwrap().to_string();
        //    v8::String::new(scope, &s).unwrap().into()
        //} else if flags.contains(ue::EClassCastFlags::CASTCLASS_FNameProperty) {
        //    let s = (ptr as *mut ue::FName).as_ref().unwrap().to_string();
        //    v8::String::new(scope, &s).unwrap().into()
        //} else if flags.contains(ue::EClassCastFlags::CASTCLASS_FArrayProperty) {
        //    let prop: &ue::FArrayProperty = &*(prop as *const _ as *const ue::FArrayProperty);
        //    let inner = prop.inner.nn().unwrap().as_ref();

        //    #[repr(C)]
        //    struct ArrayStruct {
        //        data: *const c_void,
        //        num: u32,
        //        max: u32,
        //    }

        //    let array_data = (ptr as *const ArrayStruct).as_ref().unwrap();
        //    let array = v8::Array::new(scope, array_data.num as i32);

        //    for i in 0..array_data.num {
        //        let elm = js_prop(
        //            scope,
        //            array_data
        //                .data
        //                .byte_add((i * inner.element_size as u32) as usize),
        //            inner,
        //        );
        //        array.set_index(scope, i, elm);
        //    }

        //    array.into()
        //} else {
        //    v8::String::new(scope, &format!("<TODO> {:?}", flags))
        //        .unwrap()
        //        .into()
        //}
    }
}

struct NativesArray([Option<ExecFn>; 0x100]);
unsafe extern "system" fn hook_exec<const N: usize>(
    ctx: *mut UObject,
    frame: *mut FFrame,
    ret: *mut c_void,
) {
    debug(N, ctx, frame, ret);
}
unsafe fn hook_gnatives(gnatives: &mut NativesArray) {
    seq_macro::seq!(N in 0..256 {
        (GNATIVES_OLD.0)[N] = gnatives.0[N];
        gnatives.0[N] = Some(hook_exec::<N>);
    });
}

static mut GNATIVES_OLD: NativesArray = NativesArray([None; 0x100]);
static mut NAME_CACHE: Option<HashMap<usize, String>> = None;

static ID: AtomicI32 = AtomicI32::new(1);

static SOURCES: LazyLock<Mutex<Sources>> = LazyLock::new(Default::default);
static PAUSE_STATE: Mutex<Option<PauseState>> = Mutex::new(None);
static REQUEST_PAUSE: AtomicBool = AtomicBool::new(false);
static DAP_OUTPUT: Mutex<Option<Arc<Mutex<ServerOutput<Box<dyn std::io::Write + Send>>>>>> =
    Mutex::new(None);

unsafe impl Send for Sources {}
#[derive(Default)]
struct Sources {
    ptr_to_id: HashMap<*const UFunction, i32>,
    id_to_src: HashMap<i32, Source>,
}

unsafe impl Send for PauseState {}
struct PauseState {
    ctx: *mut UObject,
    frame: *mut FFrame,
    index: usize,
}

unsafe fn debug(expr: usize, ctx: *mut UObject, frame: *mut FFrame, ret: *mut c_void) {
    if REQUEST_PAUSE.load(Ordering::SeqCst) {
        REQUEST_PAUSE.store(false, Ordering::SeqCst);

        if NAME_CACHE.is_none() {
            NAME_CACHE = Some(Default::default());
        }

        let (index, path) = {
            let stack = &*frame;
            let func = &*(stack.node as *const UFunction);

            let index = (stack.code as usize)
                .saturating_sub(func.ustruct.script.as_ptr() as usize)
                .saturating_sub(1);

            let path = NAME_CACHE
                .as_mut()
                .unwrap_unchecked()
                .entry(stack.node as usize)
                .or_insert_with(|| {
                    func.ustruct
                        .ufield
                        .uobject
                        .uobject_base_utility
                        .uobject_base
                        .get_path_name(None)
                });
            (index, path)
        };

        // populate pause state
        *PAUSE_STATE.lock().unwrap() = Some(PauseState { ctx, frame, index });

        // after PAUSE_STATE population, if DAP_OUTPUT is ready then send a pause event
        if let Some(output) = DAP_OUTPUT.lock().unwrap().as_ref() {
            let name = (*(*frame).node)
                .ustruct
                .ufield
                .uobject
                .uobject_base_utility
                .uobject_base
                .name_private
                .to_string();

            disassemble(&name, (*(*frame).node).ustruct.script.as_slice()).unwrap();

            let mut server = output.lock().unwrap();
            server
                .send_event(Event::Stopped(events::StoppedEventBody {
                    reason: types::StoppedEventReason::Pause,
                    description: Some("paused because".into()),
                    thread_id: Some(1),
                    preserve_focus_hint: None,
                    text: Some("asdf".into()),
                    all_threads_stopped: Some(true),
                    hit_breakpoint_ids: None,
                }))
                .unwrap();
        }

        // wait until PAUSE_STATE is cleared indicating execution should continue
        // TODO replace with a futex or something
        while PAUSE_STATE.lock().unwrap().is_some() {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    ((GNATIVES_OLD.0)[expr].unwrap())(ctx, frame, ret);
}

fn get_source<'a>(
    sources: &'a mut MutexGuard<'_, Sources>,
    func: *const UFunction,
) -> (i32, &'a Source) {
    let id = *sources
        .ptr_to_id
        .entry(func)
        .or_insert_with(|| ID.fetch_add(1, Ordering::SeqCst));

    (
        id,
        sources.id_to_src.entry(id).or_insert_with(|| {
            let func = unsafe { &*func };

            let obj = &func
                .ustruct
                .ufield
                .uobject
                .uobject_base_utility
                .uobject_base;

            let name = obj.name_private.to_string();
            //let path = obj.get_path_name(None);

            disassemble(&name, func.ustruct.script.as_slice()).unwrap()
        }),
    )
}
fn get_source_from_id<'a>(sources: &'a MutexGuard<'_, Sources>, id: i32) -> Option<&'a Source> {
    sources.id_to_src.get(&id)
}

fn disassemble(name: &str, bytes: &[u8]) -> Result<Source> {
    tracing::info!("dumping {name} bytes={}", bytes.len());
    std::fs::write(
        format!(
            "/home/truman/projects/drg-modding/tools/mint/bytes/{}.bin",
            name
        ),
        bytes,
    )
    .unwrap();

    let mut reader = std::io::Cursor::new(&bytes);

    let mut expr = vec![];
    let mut builder = SourceBuilder {
        offline: false,
        index: 0,
    };
    let mut out = SourceCollector::default();

    while (reader.position() as usize) < bytes.len() {
        let pos = reader.position() as usize;
        let e = Expr::read(&mut reader).unwrap();
        let lines = e.format(&mut builder);
        //dbg!(&lines);
        lines.write(&mut out, 0);
        let end_pos = reader.position() as usize;

        assert_eq!(end_pos - pos, e.size());
        assert_eq!(builder.index, end_pos);

        expr.push((reader.position(), e));
    }
    Ok(out.source)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_dis() {
        //let bytes =
        //include_bytes!("../../../bytes/ExecuteUbergraph_Bp_StartMenu_PlayerController.bin");

        for entry in std::fs::read_dir("../bytes").unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_file() {
                test_bytes(&std::fs::read(path).unwrap());
            }
        }
    }

    fn test_bytes(bytes: &[u8]) {
        let mut reader = std::io::Cursor::new(bytes);

        let mut expr = vec![];
        let mut builder = SourceBuilder {
            offline: true,
            index: 0,
        };
        let mut out = SourceCollector::default();

        while (reader.position() as usize) < bytes.len() {
            let pos = reader.position() as usize;
            let e = Expr::read(&mut reader).unwrap();
            let lines = e.format(&mut builder);
            //dbg!(&lines);
            lines.write(&mut out, 0);
            let end_pos = reader.position() as usize;

            assert_eq!(end_pos - pos, e.size());
            assert_eq!(builder.index, end_pos);

            expr.push((reader.position(), e));
        }
        println!("{}", out.source.content);
    }
}

#[derive(Debug, Clone, Copy)]
struct KProperty(u64);
impl KProperty {
    fn read(r: &mut impl std::io::Read) -> Result<Self> {
        Ok(Self(r.read_u64::<LE>()?))
    }
}
#[derive(Debug, Clone, Copy)]
struct KObject(u64);
impl KObject {
    fn read(r: &mut impl std::io::Read) -> Result<Self> {
        Ok(Self(r.read_u64::<LE>()?))
    }
}

impl ExprToken {
    fn read(r: &mut impl std::io::Read) -> Result<Self> {
        let token = r.read_u8()?;
        Self::from_repr(token).with_context(|| format!("unknown EExprToken variant = {token:X}"))
    }
}

macro_rules! expression {
    (impl $name:ident, $( $member_name:ident: [ $($member_type:tt)* ] ),* ) => {
        impl ReadExt for $name {
            fn size(&self) -> usize {
                0 $( + self.$member_name.size() )*
            }
            fn read(r: &mut impl std::io::Read) -> Result<Self> {
                Ok(Self {
                    $( $member_name: ReadExt::read(r)?, )*
                })
            }
            fn walk<F>(&mut self, mut f: F) where F: FnMut(&mut Expr) {
                $( self.$member_name.walk(&mut f); )*
            }
            fn format(&self, ctx: &mut SourceBuilder) -> SourceLine {
                let mut lines = SourceLine {
                    index: Some(ctx.index - 1),
                    content: stringify!($name).to_string(),
                    ..Default::default()
                };
                $(
                    lines.children.push(self.$member_name.format(ctx).prefix(concat!(stringify!($member_name), " = ")));
                )*
                lines
            }
        }

        expression!(no_impl $name, $( $member_name: [ $($member_type)* ] ),*);
    };
    (no_impl $name:ident, $( $member_name:ident: [ $($member_type:tt)* ] ),* ) => {
        #[derive(Debug)]
        pub struct $name {
            $( $member_name: $($member_type)*, )*
        }
    };
}

macro_rules! for_each {
    ( $( $discriminator:literal $impl:tt $name:ident { $( $member_name:ident : [ $($member_type:tt)* ] )* } )* ) => {
        #[derive(Debug, strum::FromRepr)]
        #[repr(u8)]
        pub enum ExprToken {
            $( $name = $discriminator, )*
        }
        #[derive(Debug)]
        pub enum Expr {
            $( $name($name), )*
        }
        $( expression!($impl $name, $($member_name : [$($member_type)*]),* );)*

        impl ReadExt for Expr {
            fn size(&self) -> usize {
                1 + match self {
                    $( Expr::$name(ex) => ex.size(), )*
                }
            }
            fn read(r: &mut impl std::io::Read) -> Result<Self> {
                Ok(match ExprToken::read(r)? {
                    $( ExprToken::$name => Expr::$name($name::read(r)?), )*
                })
            }
            fn walk<F>(&mut self, mut f: F) where F: FnMut(&mut Expr) {
                match self {
                    $( Expr::$name(ex) => ex.walk(&mut f), )*
                }
            }
            fn format(&self, ctx: &mut SourceBuilder) -> SourceLine {
                ctx.index += 1;
                match self {
                    $( Expr::$name(ex) => ex.format(ctx), )*
                }
            }
        }

        impl Expr {
            fn token(&self) -> ExprToken {
                match self {
                    $(
                        Expr::$name { .. } => ExprToken::$name,
                    )*
                }
            }
        }
    };
}

#[derive(Debug)]
struct KismetPropertyPointer(u64);
impl ReadExt for KismetPropertyPointer {
    fn size(&self) -> usize {
        8
    }
    fn read(r: &mut impl std::io::Read) -> Result<Self> {
        Ok(Self(r.read_u64::<LE>()?))
    }
    fn format(&self, ctx: &mut SourceBuilder) -> SourceLine {
        ctx.index += self.size();
        if ctx.offline {
            format!("FProperty({:X?})", self.0).into()
        } else {
            let prop = unsafe { (self.0 as *const FProperty).as_ref() };
            let name = if let Some(prop) = prop {
                prop.ffield.name_private.to_string()
            } else {
                "Null".to_string()
            };
            format!("FProperty({name:?})").into()
        }
    }
}
#[derive(Debug)]
struct PackageIndex(u64);
impl ReadExt for PackageIndex {
    fn size(&self) -> usize {
        8
    }
    fn read(r: &mut impl std::io::Read) -> Result<Self> {
        Ok(Self(r.read_u64::<LE>()?))
    }
    fn format(&self, ctx: &mut SourceBuilder) -> SourceLine {
        ctx.index += self.size();
        if ctx.offline {
            format!("UObject({:X?})", self.0).into()
        } else {
            let obj = unsafe { &*(self.0 as *const UObject) };
            let path = obj.uobject_base_utility.uobject_base.get_path_name(None);
            format!("UObject({path:?})").into()
        }
    }
}
#[derive(Debug)]
struct KName(u32, u32, u32);
impl ReadExt for KName {
    fn size(&self) -> usize {
        12
    }
    fn read(r: &mut impl std::io::Read) -> Result<Self> {
        Ok(Self(
            r.read_u32::<LE>()?,
            r.read_u32::<LE>()?,
            r.read_u32::<LE>()?,
        ))
    }
    fn format(&self, ctx: &mut SourceBuilder) -> SourceLine {
        ctx.index += self.size();
        if ctx.offline {
            format!("{self:X?}").into()
        } else {
            let name = FName {
                comparison_index: FNameEntryId { value: self.1 },
                number: self.2,
            };
            format!("FName({:?})", name.to_string()).into()
        }
    }
}

#[derive(Debug, strum::FromRepr)]
#[repr(u8)]
enum ECastToken {
    ObjectToInterface,
    ObjectToBool,
    InterfaceToBool,
    Max,
}
impl ReadExt for ECastToken {
    fn size(&self) -> usize {
        1
    }
    fn read(r: &mut impl std::io::Read) -> Result<Self> {
        let token = r.read_u8()?;
        Ok(Self::from_repr(token)
            .with_context(|| format!("unknown ECastToken variant = {token:X}"))
            .unwrap_or(ECastToken::Max)) // TODO fix out of range?
    }
    fn format(&self, ctx: &mut SourceBuilder) -> SourceLine {
        ctx.index += self.size();
        format!("{self:?}").into()
    }
}

#[derive(Debug)]
struct StatementIndex(u32);
impl ReadExt for StatementIndex {
    fn size(&self) -> usize {
        4
    }
    fn read(r: &mut impl std::io::Read) -> Result<Self> {
        Ok(Self(r.read_u32::<LE>()?))
    }
    fn format(&self, ctx: &mut SourceBuilder) -> SourceLine {
        ctx.index += self.size();
        self.0.to_string().into()
    }
}
impl ReadExt for i32 {
    fn size(&self) -> usize {
        4
    }
    fn read(r: &mut impl std::io::Read) -> Result<Self> {
        Ok(r.read_i32::<LE>()?)
    }
    fn format(&self, ctx: &mut SourceBuilder) -> SourceLine {
        ctx.index += self.size();
        self.to_string().into()
    }
}
impl ReadExt for u32 {
    fn size(&self) -> usize {
        4
    }
    fn read(r: &mut impl std::io::Read) -> Result<Self> {
        Ok(r.read_u32::<LE>()?)
    }
    fn format(&self, ctx: &mut SourceBuilder) -> SourceLine {
        ctx.index += self.size();
        self.to_string().into()
    }
}
impl ReadExt for u16 {
    fn size(&self) -> usize {
        2
    }
    fn read(r: &mut impl std::io::Read) -> Result<Self> {
        Ok(r.read_u16::<LE>()?)
    }
    fn format(&self, ctx: &mut SourceBuilder) -> SourceLine {
        ctx.index += self.size();
        self.to_string().into()
    }
}
impl ReadExt for u8 {
    fn size(&self) -> usize {
        1
    }
    fn read(r: &mut impl std::io::Read) -> Result<Self> {
        Ok(r.read_u8()?)
    }
    fn format(&self, ctx: &mut SourceBuilder) -> SourceLine {
        ctx.index += self.size();
        self.to_string().into()
    }
}
impl ReadExt for f32 {
    fn size(&self) -> usize {
        4
    }
    fn read(r: &mut impl std::io::Read) -> Result<Self> {
        Ok(r.read_f32::<LE>()?)
    }
    fn format(&self, ctx: &mut SourceBuilder) -> SourceLine {
        ctx.index += self.size();
        self.to_string().into()
    }
}
impl ReadExt for bool {
    fn size(&self) -> usize {
        1
    }
    fn read(r: &mut impl std::io::Read) -> Result<Self> {
        Ok(r.read_u8()? != 0)
    }
    fn format(&self, ctx: &mut SourceBuilder) -> SourceLine {
        ctx.index += self.size();
        self.to_string().into()
    }
}
impl ReadExt for FVector {
    fn size(&self) -> usize {
        12
    }
    fn read(r: &mut impl std::io::Read) -> Result<Self> {
        Ok(Self {
            x: r.read_f32::<LE>()?,
            y: r.read_f32::<LE>()?,
            z: r.read_f32::<LE>()?,
        })
    }
    fn format(&self, ctx: &mut SourceBuilder) -> SourceLine {
        ctx.index += self.size();
        format!("FVector({},{},{})", self.x, self.y, self.z).into()
    }
}
impl ReadExt for FQuat {
    fn size(&self) -> usize {
        16
    }
    fn read(r: &mut impl std::io::Read) -> Result<Self> {
        Ok(Self {
            x: r.read_f32::<LE>()?,
            y: r.read_f32::<LE>()?,
            z: r.read_f32::<LE>()?,
            w: r.read_f32::<LE>()?,
        })
    }
    fn format(&self, ctx: &mut SourceBuilder) -> SourceLine {
        ctx.index += self.size();
        format!("FQuat({},{},{},{})", self.x, self.y, self.z, self.w).into()
    }
}
impl ReadExt for FTransform {
    fn size(&self) -> usize {
        self.rotation.size() + self.translation.size() + self.scale.size()
    }
    fn read(r: &mut impl std::io::Read) -> Result<Self> {
        Ok(Self {
            rotation: ReadExt::read(r)?,
            translation: ReadExt::read(r)?,
            scale: ReadExt::read(r)?,
        })
    }
    fn format(&self, ctx: &mut SourceBuilder) -> SourceLine {
        let mut lines = SourceLine {
            content: "FTransform".into(),
            ..Default::default()
        };
        lines
            .children
            .push(self.rotation.format(ctx).prefix("rotation = "));
        lines
            .children
            .push(self.translation.format(ctx).prefix("translation = "));
        lines
            .children
            .push(self.scale.format(ctx).prefix("scale = "));
        lines
    }
}

impl<I: ReadExt> ReadExt for Box<I> {
    fn size(&self) -> usize {
        I::size(self)
    }
    fn read(r: &mut impl std::io::Read) -> Result<Self> {
        Ok(Box::new(I::read(r)?))
    }
    fn format(&self, ctx: &mut SourceBuilder) -> SourceLine {
        I::format(&self, ctx)
    }
}

#[derive(Debug)]
struct TerminatedExprList<const B: bool, const N: u8>(Vec<Expr>);
impl<const B: bool, const N: u8> ReadExt for TerminatedExprList<B, N> {
    fn size(&self) -> usize {
        let mut size = 0;
        if B {
            size += 4;
        }
        for expr in &self.0 {
            size += expr.size();
        }
        1 + size
    }
    fn read(r: &mut impl std::io::Read) -> Result<Self> {
        if B {
            let _num = r.read_u32::<LE>()?;
        }
        let mut expr = vec![];
        loop {
            let e = Expr::read(r)?;
            if e.token() as u8 == N {
                break;
            }
            expr.push(e);
        }
        Ok(Self(expr))
    }
    fn format(&self, ctx: &mut SourceBuilder) -> SourceLine {
        let mut lines = SourceLine {
            content: "list".into(),
            ..Default::default()
        };
        if B {
            ctx.index += 4;
        }
        for e in &self.0 {
            lines.children.push(e.format(ctx));
        }
        ctx.index += 1;
        lines
    }
}

trait ReadExt: std::fmt::Debug {
    fn size(&self) -> usize;
    fn read(r: &mut impl std::io::Read) -> Result<Self>
    where
        Self: Sized;
    fn walk<F>(&mut self, f: F)
    where
        F: FnMut(&mut Expr),
    {
    }
    fn format(&self, ctx: &mut SourceBuilder) -> SourceLine {
        todo!("format {:?}", self)
    }
}

impl ReadExt for ExStringConst {
    fn size(&self) -> usize {
        self.value.len() + 1
    }
    fn read(r: &mut impl std::io::Read) -> Result<Self> {
        let mut chars = vec![];
        loop {
            let c = r.read_u8()?;
            if c == 0 {
                break;
            }
            chars.push(c);
        }
        Ok(Self {
            value: String::from_utf8_lossy(&chars).to_string(),
        })
    }
    fn format(&self, ctx: &mut SourceBuilder) -> SourceLine {
        let lines = SourceLine {
            index: Some(ctx.index),
            content: format!("EX_StringConst({:?})", self.value),
            children: vec![],
            ..Default::default()
        };
        ctx.index += self.size();
        lines
    }
}
impl ReadExt for ExUnicodeStringConst {
    fn size(&self) -> usize {
        (self.value.len() + 1) * 2
    }
    fn read(r: &mut impl std::io::Read) -> Result<Self> {
        let mut chars = vec![];
        loop {
            let c = r.read_u16::<LE>()?;
            if c == 0 {
                break;
            }
            chars.push(c);
        }
        Ok(Self {
            value: String::from_utf16_lossy(&chars).to_string(),
        })
    }
    fn format(&self, ctx: &mut SourceBuilder) -> SourceLine {
        let lines = SourceLine {
            index: Some(ctx.index),
            content: format!("EX_UnicodeStringConst({:?})", self.value),
            children: vec![],
            ..Default::default()
        };
        ctx.index += self.size();
        lines
    }
}

#[derive(Debug)]
struct KismetSwitchCase {
    case_index_value_term: Expr,
    next_offset: u32,
    case_term: Expr,
}

impl ReadExt for ExSwitchValue {
    fn size(&self) -> usize {
        let mut size = 0;
        size += 2;
        size += 4;
        size += self.index_term.size();
        for c in &self.cases {
            size += c.case_index_value_term.size();
            size += 4;
            size += c.case_term.size();
        }
        size += self.default_term.size();
        size
    }
    fn read(r: &mut impl std::io::Read) -> Result<Self> {
        let mut cases = vec![];
        let num = r.read_u16::<LE>()?;
        let end_goto_offset = r.read_u32::<LE>()?;
        let index_term = Box::new(Expr::read(r)?);

        for _ in 0..num {
            cases.push(KismetSwitchCase {
                case_index_value_term: Expr::read(r)?,
                next_offset: u32::read(r)?,
                case_term: Expr::read(r)?,
            });
        }

        let default_term = Box::new(Expr::read(r)?);
        Ok(Self {
            end_goto_offset,
            index_term,
            default_term,
            cases,
        })
    }
    fn walk<F>(&mut self, mut f: F)
    where
        F: FnMut(&mut Expr),
    {
        self.index_term.walk(&mut f);
        for c in &mut self.cases {
            c.case_index_value_term.walk(&mut f);
            c.case_term.walk(&mut f);
        }
        self.default_term.walk(&mut f);
    }
    fn format(&self, ctx: &mut SourceBuilder) -> SourceLine {
        let mut lines = SourceLine {
            index: Some(ctx.index),
            content: "EX_SwitchValue".into(),
            ..Default::default()
        };
        ctx.index += 2;
        ctx.index += 4;

        lines
            .children
            .push(self.index_term.format(ctx).prefix("index = "));

        for c in &self.cases {
            let mut case_lines = SourceLine {
                content: "case".into(),
                ..Default::default()
            };
            case_lines
                .children
                .push(c.case_index_value_term.format(ctx).prefix("value = "));
            ctx.index += 4;
            case_lines
                .children
                .push(c.case_term.format(ctx).prefix("term = "));
        }

        lines
            .children
            .push(self.default_term.format(ctx).prefix("default = "));

        lines
    }
}

#[derive(Default, Debug)]
struct SourceLine {
    index: Option<usize>,
    prefix: Option<String>,
    content: String,
    children: Vec<SourceLine>,
}
struct SourceBuilder {
    offline: bool,
    index: usize,
}
#[derive(Default)]
struct SourceCollector {
    line: usize,
    source: Source,
}
#[derive(Default)]
struct Source {
    index_to_line: BTreeMap<usize, usize>,
    content: String,
}
impl SourceLine {
    fn prefix(self, prefix: impl Into<String>) -> SourceLine {
        SourceLine {
            prefix: Some(prefix.into()),
            ..self
        }
    }
    fn write(&self, out: &mut SourceCollector, indent: usize) {
        use std::fmt::Write;

        writeln!(
            out.source.content,
            "{:>8}{}{}{}",
            self.index.map(|i| format!("{i}: ")).unwrap_or_default(),
            "    ".repeat(indent),
            self.prefix.as_ref().map(|p| p.as_str()).unwrap_or(""),
            self.content
        )
        .unwrap();

        out.line += 1;

        if let Some(index) = self.index {
            out.source.index_to_line.insert(index, out.line);
        }

        for c in &self.children {
            c.write(out, indent + 1);
        }
    }
}
impl From<String> for SourceLine {
    fn from(content: String) -> Self {
        Self {
            content,
            ..Default::default()
        }
    }
}

#[derive(Debug)]
enum FScriptText {
    Empty,
    LocalizedText {
        localized_source: Expr,
        localized_key: Expr,
        localized_namespace: Expr,
    },
    InvariantText {
        invariant_literal_string: Expr,
    },
    LiteralString {
        literal_string: Expr,
    },
    StringTableEntry {
        string_table: PackageIndex,
    },
}
impl ReadExt for FScriptText {
    fn size(&self) -> usize {
        1 + match self {
            FScriptText::Empty => 0,
            FScriptText::LocalizedText {
                localized_source,
                localized_key,
                localized_namespace,
            } => localized_source.size() + localized_key.size() + localized_namespace.size(),
            FScriptText::InvariantText {
                invariant_literal_string,
            } => invariant_literal_string.size(),
            FScriptText::LiteralString { literal_string } => literal_string.size(),
            FScriptText::StringTableEntry { string_table } => string_table.size(),
        }
    }
    fn read(r: &mut impl std::io::Read) -> Result<Self> {
        // TODO
        Ok(match r.read_u8()? {
            0 => Self::Empty,
            1 => Self::LocalizedText {
                localized_source: Expr::read(r)?,
                localized_key: Expr::read(r)?,
                localized_namespace: Expr::read(r)?,
            },
            2 => Self::InvariantText {
                invariant_literal_string: Expr::read(r)?,
            },
            3 => Self::LiteralString {
                literal_string: Expr::read(r)?,
            },
            4 => Self::StringTableEntry {
                string_table: PackageIndex::read(r)?,
            },
            _ => unimplemented!("unkonwn FScriptTextVariant"),
        })
    }
    fn format(&self, ctx: &mut SourceBuilder) -> SourceLine {
        let mut lines = SourceLine {
            content: "FScriptText".into(),
            ..Default::default()
        };
        ctx.index += 1;
        match self {
            FScriptText::Empty => {}
            FScriptText::LocalizedText {
                localized_source,
                localized_key,
                localized_namespace,
            } => {
                lines
                    .children
                    .push(localized_source.format(ctx).prefix("localized_source = "));
                lines
                    .children
                    .push(localized_key.format(ctx).prefix("localized_key = "));
                lines.children.push(
                    localized_namespace
                        .format(ctx)
                        .prefix("localized_namespace = "),
                );
            }
            FScriptText::InvariantText {
                invariant_literal_string,
            } => {
                lines.children.push(
                    invariant_literal_string
                        .format(ctx)
                        .prefix("invariant_literal_string = "),
                );
            }
            FScriptText::LiteralString { literal_string } => {
                lines
                    .children
                    .push(literal_string.format(ctx).prefix("literal_string = "));
            }
            FScriptText::StringTableEntry { string_table } => {
                lines
                    .children
                    .push(string_table.format(ctx).prefix("string_table = "));
            }
        }
        lines
    }
}

for_each!(
    0x00    impl ExLocalVariable { variable: [ KismetPropertyPointer ] }
    0x01    impl ExInstanceVariable { variable: [ KismetPropertyPointer ] }
    0x02    impl ExDefaultVariable { variable: [ KismetPropertyPointer ] }
    0x04    impl ExReturn { return_expression: [ Box<Expr> ] }
    0x06    impl ExJump { code_offset: [ StatementIndex ] }
    0x07    impl ExJumpIfNot { code_offset: [ StatementIndex ] boolean_expression: [ Box<Expr> ] }
    0x09    impl ExAssert { line_number: [ u16 ] debug_mode: [ bool ] assert_expression: [ Box<Expr> ] }
    0x0B    impl ExNothing {  }
    0x0F    impl ExLet { value: [ KismetPropertyPointer ] variable: [ Box<Expr> ] expression: [ Box<Expr> ] }
    0x12    impl ExClassContext { object_expression: [ Box<Expr> ] offset: [ StatementIndex ] r_value_pointer: [ KismetPropertyPointer ] context_expression: [ Box<Expr> ] }
    0x13    impl ExMetaCast { class_ptr: [ PackageIndex ] target_expression: [ Box<Expr> ] }
    0x14    impl ExLetBool { variable_expression: [ Box<Expr> ] assignment_expression: [ Box<Expr> ] }
    0x15    impl ExEndParmValue {  }
    0x16    impl ExEndFunctionParms {  }
    0x17    impl ExSelf {  }
    0x18    impl ExSkip { code_offset: [ StatementIndex ] skip_expression: [ Box<Expr> ] }
    0x19    impl ExContext { object_expression: [ Box<Expr> ] offset: [ StatementIndex ] r_value_pointer: [ KismetPropertyPointer ] context_expression: [ Box<Expr> ] }
    0x1A    impl ExContextFailSilent { object_expression: [ Box<Expr> ] offset: [ StatementIndex ] r_value_pointer: [ KismetPropertyPointer ] context_expression: [ Box<Expr> ] }
    0x1B    impl ExVirtualFunction { virtual_function_name: [ KName ] parameters: [ TerminatedExprList<false, {ExprToken::ExEndFunctionParms as u8}> ] }
    0x1C    impl ExFinalFunction { stack_node: [ PackageIndex ] parameters: [ TerminatedExprList<false, {ExprToken::ExEndFunctionParms as u8}> ] }
    0x1D    impl ExIntConst { value: [ i32 ] }
    0x1E    impl ExFloatConst { value: [ f32 ] }
    0x1F no_impl ExStringConst { value: [ String ] }
    0x20    impl ExObjectConst { value: [ PackageIndex ] }
    0x21    impl ExNameConst { value: [ KName ] }
    0x22    impl ExRotationConst { rotator: [ FVector ] }
    0x23    impl ExVectorConst { value: [ FVector ] }
    0x24    impl ExByteConst { value: [ u8 ] }
    0x25    impl ExIntZero {  }
    0x26    impl ExIntOne {  }
    0x27    impl ExTrue {  }
    0x28    impl ExFalse {  }
    0x29    impl ExTextConst { value: [ Box<FScriptText> ] }
    0x2A    impl ExNoObject {  }
    0x2B    impl ExTransformConst { value: [ FTransform ] }
    0x2C    impl ExIntConstByte {  }
    0x2D    impl ExNoInterface {  }
    0x2E    impl ExDynamicCast { class_ptr: [ PackageIndex ] target_expression: [ Box<Expr> ] }
    0x2F    impl ExStructConst { struct_value: [ PackageIndex ] struct_size: [ i32 ] value: [ TerminatedExprList<false, {ExprToken::ExEndStructConst as u8}> ] }
    0x30    impl ExEndStructConst {  }
    0x31    impl ExSetArray { assigning_property: [ Box<Expr> ] elements: [ TerminatedExprList<false, {ExprToken::ExEndArray as u8}> ] }
    0x32    impl ExEndArray {  }
    0x33    impl ExPropertyConst { property: [ KismetPropertyPointer ] }
    0x34 no_impl ExUnicodeStringConst { value: [ String ] }
    0x35    impl ExInt64Const {  }
    0x36    impl ExUInt64Const {  }
    0x38    impl ExPrimitiveCast { conversion_type: [ ECastToken ] target: [ Box<Expr> ] }
    0x39    impl ExSetSet { set_property: [ Box<Expr> ] elements: [ TerminatedExprList<true, {ExprToken::ExEndSet as u8}> ] }
    0x3A    impl ExEndSet {  }
    0x3B    impl ExSetMap { map_property: [ Box<Expr> ] elements: [ TerminatedExprList<true, {ExprToken::ExEndMap as u8}> ] }
    0x3C    impl ExEndMap {  }
    0x3D    impl ExSetConst { inner_property: [ KismetPropertyPointer ] elements: [ TerminatedExprList<true, {ExprToken::ExEndFunctionParms as u8}> ] }
    0x3E    impl ExEndSetConst {  }
    0x3F    impl ExMapConst { key_property: [ KismetPropertyPointer ] value_property: [ KismetPropertyPointer ] elements: [ TerminatedExprList<true, {ExprToken::ExEndMapConst as u8}> ] }
    0x40    impl ExEndMapConst {  }
    0x42    impl ExStructMemberContext { struct_member_expression: [ KismetPropertyPointer ] struct_expression: [ Box<Expr> ] }
    0x43    impl ExLetMulticastDelegate { variable_expression: [ Box<Expr> ] assignment_expression: [ Box<Expr> ] }
    0x44    impl ExLetDelegate { variable_expression: [ Box<Expr> ] assignment_expression: [ Box<Expr> ] }
    0x45    impl ExLocalVirtualFunction { virtual_function_name: [ KName ] parameters: [ TerminatedExprList<false, {ExprToken::ExEndFunctionParms as u8}> ] }
    0x46    impl ExLocalFinalFunction { stack_node: [ PackageIndex ] parameters: [ TerminatedExprList<false, {ExprToken::ExEndFunctionParms as u8}> ] }
    0x48    impl ExLocalOutVariable { variable: [ KismetPropertyPointer ] }
    0x4A    impl ExDeprecatedOp4A {  }
    0x4B    impl ExInstanceDelegate { function_name: [ KName ] }
    0x4C    impl ExPushExecutionFlow { pushing_address: [ StatementIndex ] }
    0x4D    impl ExPopExecutionFlow {  }
    0x4E    impl ExComputedJump { code_offset_expression: [ Box<Expr> ] }
    0x4F    impl ExPopExecutionFlowIfNot { boolean_expression: [ Box<Expr> ] }
    0x50    impl ExBreakpoint {  }
    0x51    impl ExInterfaceContext { interface_value: [ Box<Expr> ] }
    0x52    impl ExObjToInterfaceCast { class_ptr: [ PackageIndex ] target: [ Box<Expr> ] }
    0x53    impl ExEndOfScript {  }
    0x54    impl ExCrossInterfaceCast { class_ptr: [ PackageIndex ] target: [ Box<Expr> ] }
    0x55    impl ExInterfaceToObjCast { class_ptr: [ PackageIndex ] target: [ Box<Expr> ] }
    0x5A    impl ExWireTracepoint {  }
    0x5B    impl ExSkipOffsetConst { value: [ u32 ] }
    0x5C    impl ExAddMulticastDelegate { delegate: [ Box<Expr> ] delegate_to_add: [ Box<Expr> ] }
    0x5D    impl ExClearMulticastDelegate { delegate_to_clear: [ Box<Expr> ] }
    0x5E    impl ExTracepoint {  }
    0x5F    impl ExLetObj { variable_expression: [ Box<Expr> ] assignment_expression: [ Box<Expr> ] }
    0x60    impl ExLetWeakObjPtr { variable_expression: [ Box<Expr> ] assignment_expression: [ Box<Expr> ] }
    0x61    impl ExBindDelegate { function_name: [ KName ] delegate: [ Box<Expr> ] object_term: [ Box<Expr> ] }
    0x62    impl ExRemoveMulticastDelegate { delegate: [ Box<Expr> ] delegate_to_add: [ Box<Expr> ] }
    0x63    impl ExCallMulticastDelegate { stack_node: [ PackageIndex ] parameters: [ TerminatedExprList<false, {ExprToken::ExEndFunctionParms as u8}> ] delegate: [ Box<Expr> ] }
    0x64    impl ExLetValueOnPersistentFrame { destination_property: [ KismetPropertyPointer ] assignment_expression: [ Box<Expr> ] }
    0x65    impl ExArrayConst { inner_property: [ KismetPropertyPointer ] elements: [ TerminatedExprList<true, {ExprToken::ExEndArrayConst as u8}> ] }
    0x66    impl ExEndArrayConst {  }
    0x67    impl ExSoftObjectConst { value: [ Box<Expr> ] }
    0x68    impl ExCallMath { stack_node: [ PackageIndex ] parameters: [ TerminatedExprList<false, {ExprToken::ExEndFunctionParms as u8}> ] }
    0x69 no_impl ExSwitchValue { end_goto_offset: [ u32 ] index_term: [ Box<Expr> ] default_term: [ Box<Expr> ] cases: [ Vec<KismetSwitchCase> ] }
  //0x6A    impl ExInstrumentationEvent { event_type: [ EScriptInstrumentationType ] event_name: [ Option<KName> ] }
    0x6B    impl ExArrayGetByRef { array_variable: [ Box<Expr> ] array_index: [ Box<Expr> ] }
    0x6C    impl ExClassSparseDataVariable { variable: [ KismetPropertyPointer ] }
    0x6D    impl ExFieldPathConst { value: [ Box<Expr> ] }
);
