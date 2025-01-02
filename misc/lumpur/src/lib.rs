#![feature(os_string_pathbuf_leak)]

use std::env::VarError;
use std::io::{BufRead, BufReader, LineWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::Sender;

use anyhow::Context;
use cursive::event::{Event, EventResult, Key};
use cursive::theme::{Color, ColorStyle, ColorType, Effect, Style, Theme};
use cursive::utils::markup::StyledString;
use cursive::view::{Nameable, Resizable, ScrollStrategy, Scrollable};
use cursive::views::{Button, HideableView, Layer, LinearLayout, NamedView, ScrollView, TextView, ThemedView};
use cursive::Cursive;
use parking_lot::Mutex;
use tracing::Level;
use tracing_subscriber::fmt::time::Uptime;
use unfmt::unformat;

struct EventWriter {
    stdout: std::io::Stdout,
    file: &'static Mutex<LineWriter<std::fs::File>>,
}

impl Write for EventWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.stdout.write(buf)?;
        self.file.lock().write_all(&buf[..n])?;
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.stdout.flush()
    }
}

struct LogMessage {
    timestamp: f32,
    level: Level,
    thread_name: String,
    target: String,
    filename: String,
    line_number: Option<usize>,
    message: String,
}

const EMBEDDED_KEY: &str = "__LUMPUR_EMBEDDED";
const EMBEDDED_VAL: &str = "1";
const LOG_VIEW: &str = "log_view";
const LOG_SCROLL_VIEW: &str = "log_scroll_view";


pub fn init() -> anyhow::Result<()> {
    let env_var = std::env::var(EMBEDDED_KEY);
    match env_var {
        Ok(val) => {
            if val == EMBEDDED_VAL {
                let file = std::fs::File::create("app.log")?;
                let mut file = LineWriter::new(file);
                writeln!(file, "!Program started with pid: {}", std::process::id())?;
                let file: &_ = Box::leak(Box::new(Mutex::new(file)));

                let sub = tracing_subscriber::FmtSubscriber::builder()
                    .with_file(true)
                    .with_level(true)
                    .with_line_number(true)
                    .with_ansi(false)
                    .compact()
                    .with_thread_names(true)
                    .with_timer(Uptime::default())
                    .with_writer(|| EventWriter {
                        stdout: std::io::stdout(),
                        file,
                    })
                    .finish();

                tracing::subscriber::set_global_default(sub)?;
                return Ok(());
            }
        }
        Err(VarError::NotPresent) => {}
        Err(e) => return Err(e.into()),
    }

    let max_lines: usize = std::env::var("MAX_LINES").map(|s| s.parse().unwrap_or(1000)).unwrap_or(1000);
    let mut child = Command::new(std::env::current_exe()?)
        .env(EMBEDDED_KEY, EMBEDDED_VAL)
        .args(std::env::args().skip(1))
        .stdin(Stdio::inherit())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let id = child.id();
    let (log_tx, log_rx) = std::sync::mpsc::channel::<LogMessage>();
    let stdout = BufReader::new(child.stdout.take().unwrap());
    let stderr = BufReader::new(child.stderr.take().unwrap());
    let current_dir: &_ = std::env::current_dir()?.canonicalize()?.leak();

    let make_line_f = |line_tx: Sender<LogMessage>, fallback_ty, fallback_loc: &'static str| {
        move |line: String| {
            macro_rules! default {
                () => {
                    let _ = line_tx.send(LogMessage {
                        timestamp: f32::NAN,
                        level: fallback_ty,
                        thread_name: String::new(),
                        target: String::new(),
                        filename: fallback_loc.into(),
                        line_number: None,
                        message: line,
                    });
                    return;
                };
            }
            let Some((timestamp, level_thread_name_target, filename, line_num, message)) =
                unformat!("{}s {}: {}:{}: {}", &line)
            else {
                default!();
            };
            let Some((level, thread_name, target)) =
                unformat!("{} {} {}", level_thread_name_target.trim_start())
            else {
                default!();
            };
            let mut filename = Path::new(filename)
                .canonicalize()
                .unwrap_or_else(|_| PathBuf::from(filename));
            filename = filename
                .strip_prefix(&current_dir)
                .unwrap_or(&filename)
                .to_path_buf();

            let _ = line_tx.send(LogMessage {
                timestamp: timestamp.trim_start().parse().unwrap_or(f32::NAN),
                level: match level.trim() {
                    "ERROR" => Level::ERROR,
                    "WARN" => Level::WARN,
                    "INFO" => Level::INFO,
                    "DEBUG" => Level::DEBUG,
                    "TRACE" => Level::TRACE,
                    _ => fallback_ty,
                },

                thread_name: thread_name.into(),
                target: target.into(),
                filename: filename.to_string_lossy().into(),
                line_number: Some(line_num.parse().unwrap_or(0)),
                message: message.into(),
            });
        }
    };
    let f = make_line_f(log_tx.clone(), Level::INFO, "stdout");
    std::thread::spawn(move || {
        for line in stdout.lines() {
            let Ok(line) = line else {
                break;
            };
            f(line);
        }
    });
    let f = make_line_f(log_tx, Level::ERROR, "stderr");
    std::thread::spawn(move || {
        for line in stderr.lines() {
            let Ok(line) = line else {
                break;
            };
            f(line);
        }
    });

    let ctrlc_count: &_ = Box::leak(Box::new(AtomicUsize::new(0)));
    ctrlc::set_handler(move || {
        ctrlc_count.fetch_add(1, Ordering::Relaxed);
    })
    .context("Setting Ctrl-C Handler")?;

    let mut siv = cursive::default();
    let theme = Theme::terminal_default();
    siv.set_theme(theme);

    let mut first_line_style = Style::terminal_default();
    first_line_style.color.front = ColorType::Color(Color::Rgb(50, 200, 50));

    siv.add_fullscreen_layer(
        Layer::with_color(
            LinearLayout::vertical()
                .child(TextView::new("       [PROGRAM STARTED]").style(first_line_style))
                .with_name(LOG_VIEW)
                .scrollable()
                .on_scroll_inner(move |scroll, _| {
                    if scroll.is_at_bottom() {
                        scroll.set_scroll_strategy(ScrollStrategy::StickToBottom);
                    }
                    EventResult::Consumed(None)
                })
                .scroll_strategy(ScrollStrategy::StickToBottom)
                .with_name(LOG_SCROLL_VIEW),
            ColorStyle::terminal_default(),
        )
        .full_width(),
    );
    let extra_info_visible: &_ = Box::leak(Box::new(AtomicBool::new(false)));
    let extra_info_callback = move |siv: &mut Cursive| {
        let extra_info_visible = !extra_info_visible.fetch_not(Ordering::Relaxed);

        siv.call_on_name(LOG_SCROLL_VIEW, |log_scroll_view: &mut ScrollView<LinearLayout>| {
            if extra_info_visible {
                log_scroll_view.set_scroll_strategy(ScrollStrategy::KeepRow);
            } else {
                log_scroll_view.set_scroll_strategy(ScrollStrategy::StickToBottom);
            }
        });

        siv.call_on_name(LOG_VIEW, |log_view: &mut LinearLayout| {
            for i in 0..log_view.len() {
                if let Some(_) = log_view.get_child_mut(i).unwrap().downcast_mut::<TextView>() {
                    continue;
                }
                let line: &mut ThemedView<NamedView<LinearLayout>> =
                    log_view.get_child_mut(i).unwrap().downcast_mut().unwrap();
                let line = &mut *line.get_inner_mut().get_mut();
                let top: &mut LinearLayout = line.get_child_mut(0).unwrap().downcast_mut().unwrap();
                let button_container: &mut LinearLayout =
                    top.get_child_mut(0).unwrap().downcast_mut().unwrap();

                if extra_info_visible {
                    button_container.remove_child(1);
                    let hideable: &mut HideableView<Button> = button_container
                        .get_child_mut(0)
                        .unwrap()
                        .downcast_mut()
                        .unwrap();
                    hideable.unhide();
                } else {
                    button_container.add_child(TextView::new(" "));
                    let hideable: &mut HideableView<Button> = button_container
                        .get_child_mut(0)
                        .unwrap()
                        .downcast_mut()
                        .unwrap();
                    hideable.hide();
                    if line.len() > 1 {
                        line.remove_child(1);
                    }
                }
            }
        });
    };
    siv.add_global_callback('e', extra_info_callback);
    let mut menu_style = Style::terminal_default();
    menu_style.color.back = ColorType::Color(Color::Rgb(80, 80, 80));
    siv.menubar().add_leaf(
        StyledString::styled("[E]xtras", menu_style),
        extra_info_callback,
    );
    siv.add_global_callback(Key::Esc, |s| s.select_menubar());

    let clear_callback = move |siv: &mut Cursive| {
        siv.call_on_name(LOG_VIEW, |log_view: &mut LinearLayout| {
            log_view.clear();
        });
    };
    siv.add_global_callback(Event::CtrlChar('w'), clear_callback);
    siv.menubar().add_leaf(
        StyledString::styled("Clear (Ctrl-W)", menu_style),
        clear_callback,
    );
    siv.menubar().add_leaf(
        StyledString::styled("Quit (Ctrl-C)", menu_style),
        |_| {
            ctrlc_count.fetch_add(1, Ordering::Relaxed);
        },
    );

    siv.set_autohide_menu(false);

    // We must not drop any errors past this point as the UI has spun up

    let mut siv = siv.into_runner();
    let mut last_ctrlc_count = 0;
    siv.refresh();
    let mut exit_code = 0;
    let mut line_id = 0usize;
    let mut last_message_aggregate = String::new();
    let mut last_message_count = 0usize;

    while siv.is_running() {
        siv.step();
        let mut updated = false;
        while let Ok(LogMessage {
            timestamp,
            level,
            thread_name,
            target,
            filename,
            line_number,
            message,
        }) = log_rx.try_recv()
        {
            siv.call_on_name::<LinearLayout, _, _>(LOG_VIEW, |log_view| {
                let current_message_aggregate = format!("{timestamp}{level}{thread_name}{target}{filename}{line_number:?}{message}");
                if !log_view.is_empty() {
                    if current_message_aggregate == last_message_aggregate {
                        last_message_count += 1;
                        let line: &mut ThemedView<NamedView<LinearLayout>> =
                            log_view.get_child_mut(log_view.len() - 1).unwrap().downcast_mut().unwrap();
                        let line = &mut *line.get_inner_mut().get_mut();
                        let top: &mut LinearLayout = line.get_child_mut(0).unwrap().downcast_mut().unwrap();
                        let repetition_text: &mut TextView =
                            top.get_child_mut(1).unwrap().downcast_mut().unwrap();
                        let mut style = Style::inherit_parent();
                        style.effects.insert(Effect::Bold);
                        repetition_text.set_content(StyledString::styled(format!(" x{: <4}", last_message_count), style));
                        return;
                    }
                }
                last_message_aggregate = current_message_aggregate;
                last_message_count = 1;

                if log_view.len() >= max_lines {
                    log_view.remove_child(0);
                }

                let mut theme = Theme::terminal_default();
                match level {
                    Level::ERROR => {
                        theme.palette.set_color("Primary", Color::Rgb(240, 10, 30));
                    }
                    Level::WARN => {
                        theme.palette.set_color("Primary", Color::Rgb(200, 200, 40));
                    }
                    _ => {}
                };
                let line_name = line_id.to_string();
                let line_name2 = line_name.clone();
                log_view.add_child(
                    ThemedView::new(
                        theme,
                    LinearLayout::vertical()
                            .child(
                                LinearLayout::horizontal()
                                    .child({
                                        let button = HideableView::new(
                                            Button::new_raw("+", move |siv| {
                                            siv.call_on_name(&line_name, |line: &mut LinearLayout| {
                                                if line.len() == 1 {
                                                    if let Some(line_number) = line_number {
                                                        line.add_child(
                                                            TextView::new(format!("           target: {target}    location: {filename}:{line_number}    thread: {thread_name}  "))
                                                        );
                                                    } else {
                                                        line.add_child(
                                                            TextView::new(format!("           location: {filename} (avoid using println or eprintln)"))
                                                        );
                                                    }
                                                } else {
                                                    line.remove_child(1);
                                                }
                                            });
                                        }));
                                        if extra_info_visible.load(Ordering::Relaxed) {
                                            LinearLayout::horizontal()
                                                .child(button)
                                        } else {
                                            LinearLayout::horizontal()
                                                .child(button.hidden())
                                                .child(TextView::new(" "))
                                        }
                                    })
                                    .child(TextView::new("      "))
                                    .child(TextView::new(format!("[{timestamp:.2}s {level}] {message}")))
                            )
                            .with_name(line_name2)
                    )
                );
                line_id += 1;
            });
            updated = true;
        }
        if updated {
            siv.refresh();
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                if status.success() {
                    siv.quit();
                } else {
                    exit_code = 1;
                    siv.quit();
                }
            }
            Ok(None) => {}
            Err(e) => {
                eprintln!("Failed to wait for child process: {}", e);
                exit_code = 1;
                siv.quit();
            }
        }
        let new_ctrlc_count = ctrlc_count.load(Ordering::Relaxed);
        if new_ctrlc_count != last_ctrlc_count {
            last_ctrlc_count = new_ctrlc_count;
            if new_ctrlc_count == 1 {
                let result = Command::new("kill")
                    .args(["-s", "INT", &id.to_string()])
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .output();
                match result {
                    Ok(output) => {
                        if !output.status.success() {
                            eprintln!("Prcoess did not exit successfully: {:?}", output);
                        }
                    }
                    Err(e) => {
                        eprintln!("Failed to send SIGINT to child process: {e}");
                    }
                }
            } else {
                exit_code = 1;
                if let Err(e) = child.kill() {
                    eprintln!("Failed to kill child process: {e}");
                }
                siv.quit();
            }
        }
    }
    // Very important to drop to return the terminal to its original state
    drop(siv);
    std::process::exit(exit_code);
}
