use std::{cmp::Ordering, collections::VecDeque, process::Stdio, sync::Arc};

use common::lunasim::FromLunasim;
use lunabot_ai::{LunabotAI, LunabotInterfaces};
use serde::{Deserialize, Serialize};
use urobotics::{app::Application, callbacks::caller::CallbacksStorage, define_callbacks, fn_alias, get_tokio_handle, log::{error, warn}, tokio::{io::{AsyncReadExt, AsyncWriteExt}, process::{ChildStdin, Command}, runtime::Handle}, BlockOn};

use crate::{interfaces::{drive::SimMotors, teleop::LunabaseConn}, log_teleop_messages, wait_for_ctrl_c, LunabotApp};

fn_alias! {
    pub type FromLunasimRef = CallbacksRef(FromLunasim) + Send
}
define_callbacks!(FromLunasimCallbacks => CloneFn(msg: FromLunasim) + Send);

#[derive(Clone)]
pub struct LunasimStdin(Arc<urobotics::parking_lot::Mutex<ChildStdin>>);

impl LunasimStdin {
    pub fn write(&self, bytes: &[u8]) {
        let mut stdin = self.0.lock();
        if let Err(e) = stdin
            .write_all(&u32::to_ne_bytes(bytes.len() as u32))
            .block_on()
        {
            error!("Failed to send to lunasim: {e}");
            return;
        }
        if let Err(e) = stdin.write_all(bytes).block_on() {
            error!("Failed to send to lunasim: {e}");
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct LunasimbotApp {
    #[serde(flatten)]
    app: LunabotApp,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[serde(default)]
    simulation_command: Vec<String>,
}

impl Application for LunasimbotApp {
    const APP_NAME: &'static str = "sim";

    const DESCRIPTION: &'static str = "The lunabot application in a simulated environment";

    fn run(mut self) {
        log_teleop_messages();
        
        let mut cmd = if self.simulation_command.is_empty() {
            let mut cmd = Command::new("godot");
            cmd.args(["--path", "godot/lunasim"]);
            cmd
        } else {
            let mut cmd = Command::new(self.simulation_command.remove(0));
            cmd.args(self.simulation_command);
            cmd
        };

        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let _guard = get_tokio_handle().enter();
        let child;
        let (lunasim_stdin, from_lunasim_ref) = match cmd.spawn() {
            Ok(tmp) => {
                child = tmp;
                let stdin = child.stdin.unwrap();
                let mut stdout = child.stdout.unwrap();
                let mut stderr = child.stderr.unwrap();
                macro_rules! handle_err {
                    ($msg: literal, $err: ident) => {{
                        match $err.kind() {
                            std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::Other | std::io::ErrorKind::UnexpectedEof => {}
                            _ => {
                                error!(target: "lunasim", "Faced the following error while reading {}: {}", $msg, $err);
                            }
                        }
                        break;
                    }}
                }

                let handle = Handle::current();
                handle.spawn(async move {
                    let mut bytes = Vec::with_capacity(1024);
                    let mut buf = [0u8; 1024];

                    loop {
                        if bytes.len() == bytes.capacity() {
                            bytes.reserve(bytes.len());
                        }
                        match stderr.read(&mut buf).await {
                            Ok(0) => {}
                            Ok(n) => {
                                bytes.extend_from_slice(&buf[0..n]);
                                if let Ok(string) = std::str::from_utf8(&bytes) {
                                    if let Some(i) = string.find('\n') {
                                        warn!(target: "lunasim", "{}", &string[0..i]);
                                        bytes.drain(0..=i);
                                    }
                                }
                            }
                            Err(e) => handle_err!("stderr", e),
                        }
                    }
                });

                let mut callbacks = FromLunasimCallbacks::default();
                let callbacks_ref = callbacks.get_ref();

                handle.spawn(async move {
                    {
                        let mut bytes = VecDeque::with_capacity(7);
                        let mut buf = [0u8];
                        loop {
                            match stdout.read_exact(&mut buf).await {
                                Ok(0) => unreachable!("godot program should not exit"),
                                Ok(_) => {
                                    bytes.push_back(buf[0]);
                                }
                                Err(e) => error!(target: "lunasim", "Faced the following error while reading stdout: {e}"),
                            }
                            match bytes.len().cmp(&6) {
                                Ordering::Equal => {}
                                Ordering::Greater => {bytes.pop_front();}
                                Ordering::Less => continue,
                            }
                            if bytes == b"READY\n" {
                                break;
                            }
                        }
                    }
                    let mut size_buf = [0u8; 4];
                    let mut bytes = Vec::with_capacity(1024);
                    loop {
                        let size = match stdout.read_exact(&mut size_buf).await {
                            Ok(_) => u32::from_ne_bytes(size_buf),
                            Err(e) => handle_err!("stdout", e)
                        };
                        bytes.resize(size as usize, 0u8);
                        match stdout.read_exact(&mut bytes).await {
                            Ok(_) => {},
                            Err(e) => handle_err!("stdout", e)
                        }

                        match FromLunasim::try_from(bytes.as_slice()) {
                            Ok(msg) => {
                                callbacks.call(msg);
                            }
                            Err(e) => {
                                error!(target: "lunasim", "Failed to deserialize from lunasim: {e}");
                                continue;
                            }
                        }
                    }
                });

                (LunasimStdin(Arc::new(stdin.into())), callbacks_ref)
            }
            Err(e) => {
                error!("Failed to run simulation command: {e}");
                return;
            }
        };

        LunabotAI::from(|interfaces: Option<LunabotInterfaces<SimMotors, _, _, LunabaseConn>>| {
            Ok(LunabotInterfaces {
                teleop: LunabaseConn::new(self.app.lunabase_address).map_err(|e| {
                    error!("Failed to connect to lunabase: {e}");
                    ()
                })?,
                drive: SimMotors::new(lunasim_stdin.clone(), from_lunasim_ref.clone())
            })
        }).spawn();
        
        wait_for_ctrl_c();
    }
}