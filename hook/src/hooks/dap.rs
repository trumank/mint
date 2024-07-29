use std::collections::HashMap;
use std::ffi::c_void;
use std::io::{BufReader, BufWriter};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

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
    UnhandledCommandError,

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
                    supports_evaluate_for_hovers: Some(true),
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
                                //let name = ufunc
                                //    .ustruct
                                //    .ufield
                                //    .uobject
                                //    .uobject_base_utility
                                //    .uobject_base
                                //    .get_path_name(None);

                                let name = ufunc
                                    .ustruct
                                    .ufield
                                    .uobject
                                    .uobject_base_utility
                                    .uobject_base
                                    .name_private
                                    .to_string();

                                frames.push(types::StackFrame {
                                    id: frame as i64,
                                    name,
                                    source: None,
                                    line: 0,
                                    column: 0,
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
            Command::Scopes(ref cmd) => {
                // TODO
                let frame = unsafe { &*(cmd.frame_id as *const FFrame) };
                server.respond(req.success(ResponseBody::Scopes(responses::ScopesResponse {
                    scopes: vec![types::Scope {
                        name: "Locals".into(),
                        presentation_hint: Some(types::ScopePresentationhint::Locals),
                        variables_reference: 0,
                        named_variables: None,
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
            Command::Continue(_) => {
                PAUSE_STATE.lock().unwrap().take();
                server.respond(req.success(ResponseBody::Continue(
                    responses::ContinueResponse {
                        all_threads_continued: None,
                    },
                )))?;
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
            _ => {
                return Err(Box::new(MyAdapterError::UnhandledCommandError));
            }
        }
    }
    Ok(())
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

static PAUSE_STATE: Mutex<Option<PauseState>> = Mutex::new(None);
static REQUEST_PAUSE: AtomicBool = AtomicBool::new(false);
static DAP_OUTPUT: Mutex<Option<Arc<Mutex<ServerOutput<Box<dyn std::io::Write + Send>>>>>> =
    Mutex::new(None);

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

            let index = (stack.code as usize).wrapping_sub(func.ustruct.script.as_ptr() as usize);

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

            let name = (*(*frame).node)
                .ustruct
                .ufield
                .uobject
                .uobject_base_utility
                .uobject_base
                .name_private
                .to_string();

            disassemble(&name, (*(*frame).node).ustruct.script.as_slice());
        }

        // wait until PAUSE_STATE is cleared indicating execution should continue
        // TODO replace with a futex or something
        while PAUSE_STATE.lock().unwrap().is_some() {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    ((GNATIVES_OLD.0)[expr].unwrap())(ctx, frame, ret);
}

fn disassemble(name: &str, bytes: &[u8]) {
    tracing::info!("dumping {name} bytes={}", bytes.len());
    std::fs::write(
        format!(
            "/home/truman/projects/drg-modding/tools/mint/bytes/{}.bin",
            name
        ),
        bytes,
    )
    .unwrap();
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_dis() {
        let bytes =
            include_bytes!("../../../bytes/ExecuteUbergraph_Bp_StartMenu_PlayerController.bin");

        let mut reader = std::io::Cursor::new(bytes);

        loop {
            let expr = EExprToken::read(&mut reader).unwrap();
            dbg!(expr);
        }
    }
}

macro_rules! expr {
    (
        $(
            $(#[$attr:meta])*
            enum $enum_name:ident {
                $(
                    $name:ident = $value:literal
                ),* $(,)?
            }
        )*
    ) => {
        $(
            $(#[$attr])*
            enum $enum_name {
                $(
                    $name = $value,
                )*
            }

            impl EExpr {
                fn token(&self) -> EExprToken {
                    match self {
                        $(
                            EExpr::$name { .. } => EExprToken::$name,
                        )*
                    }
                }
            }
        )*
    };
}

expr! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, strum::FromRepr)]
    #[repr(u8)]
    enum EExprToken {
        ExLocalVariable = 0x00,
        ExInstanceVariable = 0x01,
        ExDefaultVariable = 0x02,
        ExReturn = 0x04,
        ExJump = 0x06,
        ExJumpIfNot = 0x07,
        ExAssert = 0x09,
        ExNothing = 0x0B,
        ExLet = 0x0F,
        ExClassContext = 0x12,
        ExMetaCast = 0x13,
        ExLetBool = 0x14,
        ExEndParmValue = 0x15,
        ExEndFunctionParms = 0x16,
        ExSelf = 0x17,
        ExSkip = 0x18,
        ExContext = 0x19,
        ExContextFailSilent = 0x1A,
        ExVirtualFunction = 0x1B,
        ExFinalFunction = 0x1C,
        ExIntConst = 0x1D,
        ExFloatConst = 0x1E,
        ExStringConst = 0x1F,
        ExObjectConst = 0x20,
        ExNameConst = 0x21,
        ExRotationConst = 0x22,
        ExVectorConst = 0x23,
        ExByteConst = 0x24,
        ExIntZero = 0x25,
        ExIntOne = 0x26,
        ExTrue = 0x27,
        ExFalse = 0x28,
        ExTextConst = 0x29,
        ExNoObject = 0x2A,
        ExTransformConst = 0x2B,
        ExIntConstByte = 0x2C,
        ExNoInterface = 0x2D,
        ExDynamicCast = 0x2E,
        ExStructConst = 0x2F,
        ExEndStructConst = 0x30,
        ExSetArray = 0x31,
        ExEndArray = 0x32,
        ExPropertyConst = 0x33,
        ExUnicodeStringConst = 0x34,
        ExInt64Const = 0x35,
        ExUInt64Const = 0x36,
        ExDoubleConst = 0x37,
        ExCast = 0x38,
        ExSetSet = 0x39,
        ExEndSet = 0x3A,
        ExSetMap = 0x3B,
        ExEndMap = 0x3C,
        ExSetConst = 0x3D,
        ExEndSetConst = 0x3E,
        ExMapConst = 0x3F,
        ExEndMapConst = 0x40,
        ExVector3fConst = 0x41,
        ExStructMemberContext = 0x42,
        ExLetMulticastDelegate = 0x43,
        ExLetDelegate = 0x44,
        ExLocalVirtualFunction = 0x45,
        ExLocalFinalFunction = 0x46,
        ExLocalOutVariable = 0x48,
        ExDeprecatedOp4A = 0x4A,
        ExInstanceDelegate = 0x4B,
        ExPushExecutionFlow = 0x4C,
        ExPopExecutionFlow = 0x4D,
        ExComputedJump = 0x4E,
        ExPopExecutionFlowIfNot = 0x4F,
        ExBreakpoint = 0x50,
        ExInterfaceContext = 0x51,
        ExObjToInterfaceCast = 0x52,
        ExEndOfScript = 0x53,
        ExCrossInterfaceCast = 0x54,
        ExInterfaceToObjCast = 0x55,
        ExWireTracepoint = 0x5A,
        ExSkipOffsetConst = 0x5B,
        ExAddMulticastDelegate = 0x5C,
        ExClearMulticastDelegate = 0x5D,
        ExTracepoint = 0x5E,
        ExLetObj = 0x5F,
        ExLetWeakObjPtr = 0x60,
        ExBindDelegate = 0x61,
        ExRemoveMulticastDelegate = 0x62,
        ExCallMulticastDelegate = 0x63,
        ExLetValueOnPersistentFrame = 0x64,
        ExArrayConst = 0x65,
        ExEndArrayConst = 0x66,
        ExSoftObjectConst = 0x67,
        ExCallMath = 0x68,
        ExSwitchValue = 0x69,
        ExInstrumentationEvent = 0x6A,
        ExArrayGetByRef = 0x6B,
        ExClassSparseDataVariable = 0x6C,
        ExFieldPathConst = 0x6D,
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

#[derive(Debug, Clone)]
enum EExpr {
    ExLocalVariable {
        variable: KProperty,
    },
    ExInstanceVariable,
    ExDefaultVariable,
    ExReturn,
    ExJump,
    ExJumpIfNot,
    ExAssert,
    ExNothing,
    ExLet,
    ExClassContext,
    ExMetaCast,
    ExLetBool,
    ExEndParmValue,
    ExEndFunctionParms,
    ExSelf,
    ExSkip,
    ExContext,
    ExContextFailSilent,
    ExVirtualFunction,
    ExFinalFunction,
    ExIntConst,
    ExFloatConst,
    ExStringConst,
    ExObjectConst,
    ExNameConst,
    ExRotationConst,
    ExVectorConst,
    ExByteConst,
    ExIntZero,
    ExIntOne,
    ExTrue,
    ExFalse,
    ExTextConst,
    ExNoObject,
    ExTransformConst,
    ExIntConstByte,
    ExNoInterface,
    ExDynamicCast,
    ExStructConst,
    ExEndStructConst,
    ExSetArray,
    ExEndArray,
    ExPropertyConst,
    ExUnicodeStringConst,
    ExInt64Const,
    ExUInt64Const,
    ExDoubleConst,
    ExCast,
    ExSetSet,
    ExEndSet,
    ExSetMap,
    ExEndMap,
    ExSetConst,
    ExEndSetConst,
    ExMapConst,
    ExEndMapConst,
    ExVector3fConst,
    ExStructMemberContext,
    ExLetMulticastDelegate,
    ExLetDelegate,
    ExLocalVirtualFunction,
    ExLocalFinalFunction,
    ExLocalOutVariable,
    ExDeprecatedOp4A,
    ExInstanceDelegate,
    ExPushExecutionFlow {
        offset: u32,
    },
    ExPopExecutionFlow,
    ExComputedJump {
        offset: Box<EExpr>,
    },
    ExPopExecutionFlowIfNot,
    ExBreakpoint,
    ExInterfaceContext,
    ExObjToInterfaceCast,
    ExEndOfScript,
    ExCrossInterfaceCast,
    ExInterfaceToObjCast,
    ExWireTracepoint,
    ExSkipOffsetConst,
    ExAddMulticastDelegate,
    ExClearMulticastDelegate,
    ExTracepoint,
    ExLetObj {
        variable_expression: Box<EExpr>,
        assignment_expression: Box<EExpr>,
    },
    ExLetWeakObjPtr,
    ExBindDelegate,
    ExRemoveMulticastDelegate,
    ExCallMulticastDelegate,
    ExLetValueOnPersistentFrame,
    ExArrayConst,
    ExEndArrayConst,
    ExSoftObjectConst,
    ExCallMath {
        stack_node: KObject, // UFunction
        parameters: Vec<EExpr>,
    },
    ExSwitchValue,
    ExInstrumentationEvent,
    ExArrayGetByRef,
    ExClassSparseDataVariable,
    ExFieldPathConst,
}

impl EExprToken {
    fn read(r: &mut impl std::io::Read) -> Result<EExpr> {
        fn read_until(r: &mut impl std::io::Read, end: EExprToken) -> Result<Vec<EExpr>> {
            let mut expr = vec![];
            loop {
                let e = EExprToken::read(r)?;
                if e.token() == end {
                    break;
                }
                expr.push(e);
            }
            Ok(expr)
        }

        let token = r.read_u8()?;
        let token = Self::from_repr(token)
            .with_context(|| format!("unknown EExprToken variant = {token:X}"))?;

        Ok(match token {
            EExprToken::ExLocalVariable => EExpr::ExLocalVariable {
                variable: KProperty::read(r)?,
            },
            EExprToken::ExInstanceVariable => todo!(),
            EExprToken::ExDefaultVariable => todo!(),
            EExprToken::ExReturn => todo!(),
            EExprToken::ExJump => todo!(),
            EExprToken::ExJumpIfNot => todo!(),
            EExprToken::ExAssert => todo!(),
            EExprToken::ExClassContext => todo!(),
            EExprToken::ExMetaCast => todo!(),

            EExprToken::ExNothing => EExpr::ExNothing,
            EExprToken::ExEndOfScript => EExpr::ExEndOfScript,
            EExprToken::ExEndFunctionParms => EExpr::ExEndFunctionParms,
            EExprToken::ExEndStructConst => EExpr::ExEndStructConst,
            EExprToken::ExEndArray => EExpr::ExEndArray,
            EExprToken::ExEndArrayConst => EExpr::ExEndArrayConst,
            EExprToken::ExEndSet => EExpr::ExEndSet,
            EExprToken::ExEndMap => EExpr::ExEndMap,
            EExprToken::ExEndSetConst => EExpr::ExEndSetConst,
            EExprToken::ExEndMapConst => EExpr::ExEndMapConst,
            EExprToken::ExIntZero => EExpr::ExIntZero,
            EExprToken::ExIntOne => EExpr::ExIntOne,
            EExprToken::ExTrue => EExpr::ExTrue,
            EExprToken::ExFalse => EExpr::ExFalse,
            EExprToken::ExNoObject => EExpr::ExNoObject,
            EExprToken::ExNoInterface => EExpr::ExNoInterface,
            EExprToken::ExSelf => EExpr::ExSelf,
            EExprToken::ExEndParmValue => EExpr::ExEndParmValue,
            EExprToken::ExPopExecutionFlow => EExpr::ExPopExecutionFlow,
            EExprToken::ExDeprecatedOp4A => EExpr::ExDeprecatedOp4A,

            EExprToken::ExSkip => todo!(),
            EExprToken::ExContext => todo!(),
            EExprToken::ExContextFailSilent => todo!(),
            EExprToken::ExVirtualFunction => todo!(),
            EExprToken::ExFinalFunction => todo!(),
            EExprToken::ExIntConst => todo!(),
            EExprToken::ExFloatConst => todo!(),
            EExprToken::ExStringConst => todo!(),
            EExprToken::ExObjectConst => todo!(),
            EExprToken::ExNameConst => todo!(),
            EExprToken::ExRotationConst => todo!(),
            EExprToken::ExVectorConst => todo!(),
            EExprToken::ExByteConst => todo!(),
            EExprToken::ExTextConst => todo!(),
            EExprToken::ExTransformConst => todo!(),
            EExprToken::ExIntConstByte => todo!(),
            EExprToken::ExDynamicCast => todo!(),
            EExprToken::ExStructConst => todo!(),
            EExprToken::ExSetArray => todo!(),
            EExprToken::ExPropertyConst => todo!(),
            EExprToken::ExUnicodeStringConst => todo!(),
            EExprToken::ExInt64Const => todo!(),
            EExprToken::ExUInt64Const => todo!(),
            EExprToken::ExDoubleConst => todo!(),
            EExprToken::ExCast => todo!(),
            EExprToken::ExSetSet => todo!(),
            EExprToken::ExSetMap => todo!(),
            EExprToken::ExSetConst => todo!(),
            EExprToken::ExMapConst => todo!(),
            EExprToken::ExVector3fConst => todo!(),
            EExprToken::ExStructMemberContext => todo!(),
            EExprToken::ExLocalVirtualFunction => todo!(),
            EExprToken::ExLocalFinalFunction => todo!(),
            EExprToken::ExLocalOutVariable => todo!(),
            EExprToken::ExInstanceDelegate => todo!(),
            EExprToken::ExPushExecutionFlow => EExpr::ExPushExecutionFlow {
                offset: r.read_u32::<LE>()?,
            },
            EExprToken::ExComputedJump => EExpr::ExComputedJump {
                offset: Box::new(Self::read(r)?),
            },
            EExprToken::ExPopExecutionFlowIfNot => todo!(),
            EExprToken::ExBreakpoint => todo!(),
            EExprToken::ExInterfaceContext => todo!(),
            EExprToken::ExObjToInterfaceCast => todo!(),
            EExprToken::ExCrossInterfaceCast => todo!(),
            EExprToken::ExInterfaceToObjCast => todo!(),
            EExprToken::ExWireTracepoint => todo!(),
            EExprToken::ExSkipOffsetConst => todo!(),
            EExprToken::ExAddMulticastDelegate => todo!(),
            EExprToken::ExClearMulticastDelegate => todo!(),
            EExprToken::ExTracepoint => todo!(),

            EExprToken::ExLet => todo!(),
            EExprToken::ExLetBool => todo!(),
            EExprToken::ExLetObj => EExpr::ExLetObj {
                variable_expression: Box::new(Self::read(r)?),
                assignment_expression: Box::new(Self::read(r)?),
            },
            EExprToken::ExLetMulticastDelegate => todo!(),
            EExprToken::ExLetDelegate => todo!(),
            EExprToken::ExLetWeakObjPtr => todo!(),

            EExprToken::ExBindDelegate => todo!(),
            EExprToken::ExRemoveMulticastDelegate => todo!(),
            EExprToken::ExCallMulticastDelegate => todo!(),
            EExprToken::ExLetValueOnPersistentFrame => todo!(),
            EExprToken::ExArrayConst => todo!(),
            EExprToken::ExSoftObjectConst => todo!(),
            EExprToken::ExCallMath => EExpr::ExCallMath {
                stack_node: KObject::read(r)?,
                parameters: read_until(r, EExprToken::ExEndFunctionParms)?,
            },
            EExprToken::ExSwitchValue => todo!(),
            EExprToken::ExInstrumentationEvent => todo!(),
            EExprToken::ExArrayGetByRef => todo!(),
            EExprToken::ExClassSparseDataVariable => todo!(),
            EExprToken::ExFieldPathConst => todo!(),
        })
    }
}
