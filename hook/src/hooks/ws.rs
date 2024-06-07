use futures_util::{SinkExt, StreamExt as _};
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::{Mutex, MutexGuard, OnceLock};
use tokio::sync::mpsc;

use crate::hooks::ExecFn;
use crate::ue::{self, FString};

pub fn kismet_hooks() -> &'static [(&'static str, ExecFn)] {
    &[
        (
            "/Game/_mint/WebSocketConnection.WebSocketConnection_C:Connect",
            exec_connect as ExecFn,
        ),
        (
            "/Game/_mint/WebSocketConnection.WebSocketConnection_C:Send",
            exec_send as ExecFn,
        ),
        (
            "/Game/_mint/WebSocketConnection.WebSocketConnection_C:GetEvent",
            exec_get_event as ExecFn,
        ),
    ]
}

// TODO potential bug if object gets freed and another gets allocated with same address
// need to implement and leverage TWeakPtr
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct Handle(*mut ue::UObject);
unsafe impl Send for Handle {}

struct State {
    rt_handle: tokio::runtime::Handle,
    handles: HashMap<Handle, Connection>,
}
impl Default for State {
    fn default() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();

            tx.send(rt.handle().clone()).unwrap();

            rt.block_on(futures::future::pending::<()>())
        });
        let rt_handle = rx.recv().unwrap();
        tracing::info!("created tokio runtime");
        Self {
            rt_handle,
            handles: Default::default(),
        }
    }
}

struct Connection {
    tx: mpsc::Sender<String>,
    rx: mpsc::Receiver<Message>,
}

struct Message {
    type_: EventType,
    data: String,
}

#[derive(Default, Debug, Clone, Copy)]
#[repr(u8)]
enum EventType {
    #[default]
    None,
    Open,
    Close,
    Message,
    Error,
}

static STATE: OnceLock<Mutex<State>> = OnceLock::new();
fn get_state() -> MutexGuard<'static, State> {
    STATE.get_or_init(Default::default).lock().unwrap()
}

unsafe extern "system" fn exec_connect(
    context: *mut ue::UObject,
    stack: *mut ue::kismet::FFrame,
    _result: *mut c_void,
) {
    let stack = stack.as_mut().unwrap();

    let url: FString = stack.arg();
    let url = url.to_string();

    tracing::info!("connecting to {url}");

    let handle = Handle(context);

    let state = get_state();
    state.rt_handle.spawn(async move {
        let (socket, response) = tokio_tungstenite::connect_async(url)
            .await
            .expect("Can't connect");

        tracing::info!("Connected to the server");
        tracing::info!("Response HTTP code: {}", response.status());
        tracing::info!("Response contains the following headers:");
        for (ref header, _value) in response.headers() {
            tracing::info!("* {}", header);
        }

        let (recv_tx, recv_rx) = mpsc::channel(10);
        let (send_tx, mut send_rx) = mpsc::channel(10);

        // TODO handle cleaning up existing connection
        get_state().handles.insert(
            handle,
            Connection {
                tx: send_tx,
                rx: recv_rx,
            },
        );

        let (mut write, mut read) = socket.split();

        let a = tokio::task::spawn(async move {
            while let Some(msg) = send_rx.recv().await {
                write.send(msg.into()).await.unwrap();
            }
        });

        let b = tokio::task::spawn(async move {
            while let Some(msg) = read.next().await {
                let msg = match msg {
                    Ok(data) => Message {
                        type_: EventType::Message,
                        data: data.to_string(),
                    },
                    Err(err) => Message {
                        type_: EventType::Error,
                        data: err.to_string(),
                    },
                };
                // TODO consider if sending can fail (because connection closed)
                recv_tx.send(msg).await.unwrap();
            }
        });

        a.await.unwrap();
        b.await.unwrap();
    });

    if !stack.code.is_null() {
        stack.code = stack.code.add(1);
    }
}
unsafe extern "system" fn exec_send(
    context: *mut ue::UObject,
    stack: *mut ue::kismet::FFrame,
    _result: *mut c_void,
) {
    let stack = stack.as_mut().unwrap();

    let message: FString = stack.arg();
    let message = message.to_string();

    tracing::info!("sending message {message}");

    if let Some(connection) = get_state().handles.get_mut(&Handle(context)) {
        connection.tx.try_send(message).unwrap();
    } else {
        tracing::warn!("tried to send data but connection does not exist")
    }

    if !stack.code.is_null() {
        stack.code = stack.code.add(1);
    }
}
unsafe extern "system" fn exec_get_event(
    context: *mut ue::UObject,
    stack: *mut ue::kismet::FFrame,
    _result: *mut c_void,
) {
    let stack = stack.as_mut().unwrap();

    stack.arg::<EventType>();
    let type_: &mut EventType = &mut *(stack.most_recent_property_address as *mut EventType);
    *type_ = EventType::None;

    drop(stack.arg::<FString>());
    let data: &mut FString = &mut *(stack.most_recent_property_address as *mut FString);
    *data = FString::new();

    if let Some(connection) = get_state().handles.get_mut(&Handle(context)) {
        if let Ok(msg) = connection.rx.try_recv() {
            *type_ = msg.type_;
            *data = msg.data.as_str().into();
        }
    } else {
        tracing::warn!("tried to recv data but connection does not exist")
    }

    if !stack.code.is_null() {
        stack.code = stack.code.add(1);
    }
}
