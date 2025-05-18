use std::io::{BufReader, BufWriter, Read, StdoutLock};

use lunabot_ai_common::{FromAI, FromHost, ParseError, AI_HEARTBEAT_RATE};
use tokio::{sync::mpsc::Receiver, time::Instant};

pub struct HostHandle {
    from_host: Receiver<FromHost>,
    stdout: BufWriter<StdoutLock<'static>>,
    last_heartbeat: Instant
}

impl HostHandle {
    pub fn new() -> Self {
        let (from_host_tx, from_host) = tokio::sync::mpsc::channel(32);

        std::thread::spawn(move || {
            let mut stdin = BufReader::new(std::io::stdin().lock());
            let mut bytes = vec![];
            let mut buf = [0u8; 256];
            let mut necessary_bytes = 1usize;

            'main: loop {
                match stdin.read(&mut buf) {
                    Ok(n) => {
                        bytes.extend_from_slice(&buf[0..n]);
                        
                        loop {
                            if bytes.len() < necessary_bytes {
                                break;
                            }
                            match FromHost::parse(&bytes) {
                                Ok((msg, n)) => {
                                    bytes.drain(0..n);
                                    if let Err(e) = from_host_tx.try_send(msg) {
                                        eprintln!("Maxed out queue");
                                        if from_host_tx.blocking_send(e.into_inner()).is_err() {
                                            break 'main;
                                        }
                                    }
                                    necessary_bytes = 1;
                                }
                                Err(ParseError::InvalidData) => {
                                    std::process::exit(1);
                                }
                                Err(ParseError::NotEnoughBytes { bytes_needed }) => {
                                    necessary_bytes = bytes_needed;
                                    break;
                                }
                            }
                        }
                    }
                    Err(_) => std::process::exit(0)
                }
            }
        });

        Self {
            from_host,
            stdout: BufWriter::new(std::io::stdout().lock()),
            last_heartbeat: Instant::now()
        }
    }

    pub async fn read_from_host(&mut self) -> FromHost {
        let heartbeat = async {
            loop {
                let next_instant = self.last_heartbeat + AI_HEARTBEAT_RATE;
                tokio::time::sleep_until(next_instant).await;
                self.last_heartbeat = next_instant;
                let _ = FromAI::Heartbeat.write_into(&mut self.stdout);
            }
        };
        tokio::select! {
            option = self.from_host.recv() => option.unwrap(),
            _ = heartbeat => unreachable!()
        }
    }

    pub fn write_to_host(&mut self, msg: FromAI) {
        let _ = msg.write_into(&mut self.stdout);
    }
}
