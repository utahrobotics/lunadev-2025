#![feature(os_string_pathbuf_leak)]

use std::any::type_name;
use std::env::VarError;
use std::io::{BufRead, BufReader, LineWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use chrono::{Datelike, Timelike};
use config::Configuration;
use cursive::event::{Event, EventResult, Key};
use cursive::theme::{Color, ColorStyle, ColorType, Effect, Style, Theme};
use cursive::utils::markup::StyledString;
use cursive::view::{Nameable, Resizable, ScrollStrategy, Scrollable};
use cursive::views::{
    Button, HideableView, Layer, LinearLayout, NamedView, ScrollView, TextView, ThemedView,
};
use cursive::Cursive;
use log::{log_write_thread, make_line_f, LogMessage};
use raw_sync::events::{EventInit, EventState};
use regex::RegexSet;
use shared_memory::{ShmemConf, ShmemError};
pub use subprocess::set_on_exit;
use subprocess::{subprocess_fn, EMBEDDED_KEY, EMBEDDED_VAL, SHMEM_VAR_KEY};
use tracing::Level;

pub mod config;
mod log;
mod subprocess;

const LOG_VIEW: &str = "log_view";
const LOG_SCROLL_VIEW: &str = "log_scroll_view";

#[derive(Default)]
pub enum NewWorkingDirectory {
    Current,
    Custom(PathBuf),
    #[default]
    Automatic,
}

impl NewWorkingDirectory {
    pub fn into_path_buf(self) -> Option<PathBuf> {
        match self {
            NewWorkingDirectory::Current => None,
            NewWorkingDirectory::Custom(path) => Some(path),
            NewWorkingDirectory::Automatic => {
                let datetime = chrono::Local::now();
                let mut out = PathBuf::from("output");
                out.push(format!(
                    "{}-{:0>2}-{:0>2}",
                    datetime.year(),
                    datetime.month(),
                    datetime.day()
                ));
                out.push(format!(
                    "{:0>2};{:0>2};{:0>2}",
                    datetime.hour(),
                    datetime.minute(),
                    datetime.second()
                ));
                Some(out)
            }
        }
    }
}

pub enum PathReference {
    Copy(PathBuf),
    Symlink(PathBuf),
}

pub struct LumpurBuilder {
    pub new_working_directory: NewWorkingDirectory,
    pub path_reference: Vec<PathReference>,
    pub default_commands: bool,
    // pub ignores: FxHashMap<String, (Level, bool)>,
    pub console_ignores: RegexSet,
    pub total_ignores: RegexSet,
}

impl Default for LumpurBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl LumpurBuilder {
    pub fn new() -> Self {
        Self {
            new_working_directory: NewWorkingDirectory::default(),
            path_reference: vec![PathReference::Copy(PathBuf::from("app-config.toml"))],
            default_commands: true,
            console_ignores: Default::default(),
            total_ignores: Default::default(),
        }
    }

    pub fn new_working_directory(mut self, new_working_directory: NewWorkingDirectory) -> Self {
        self.new_working_directory = new_working_directory;
        self
    }

    pub fn symlink_path(mut self, path: impl AsRef<Path>) -> Self {
        self.path_reference
            .push(PathReference::Symlink(path.as_ref().to_path_buf()));
        self
    }

    pub fn copy_file(mut self, path: impl AsRef<Path>) -> Self {
        self.path_reference
            .push(PathReference::Copy(path.as_ref().to_path_buf()));
        self
    }

    fn into_regex_set(
        regexes_levels: impl IntoIterator<Item = (impl Into<String>, Level)>,
    ) -> RegexSet {
        RegexSet::new(regexes_levels.into_iter().map(|(regex, level)| {
            let mut regex = regex.into();
            const LEVELS: [Level; 5] = [
                Level::ERROR,
                Level::WARN,
                Level::INFO,
                Level::DEBUG,
                Level::TRACE,
            ];
            let index = LEVELS
                .iter()
                .position(|&l| l == level)
                .expect("Invalid level");
            regex.push_str("=(");
            regex.push_str(&level.to_string());
            for i in (index + 1)..LEVELS.len() {
                regex.push_str("|");
                regex.push_str(&LEVELS[i].to_string());
            }
            regex.push_str(")");
            regex
        }))
        .expect("Failed to create regex set")
    }

    pub fn set_console_ignores(
        mut self,
        regexes_levels: impl IntoIterator<Item = (impl Into<String>, Level)>,
    ) -> Self {
        self.console_ignores = Self::into_regex_set(regexes_levels);
        self
    }

    pub fn set_total_ignores(
        mut self,
        regexes_levels: impl IntoIterator<Item = (impl Into<String>, Level)>,
    ) -> Self {
        self.total_ignores = Self::into_regex_set(regexes_levels);
        self
    }

    // pub fn add_ignore(mut self, target: impl Into<String>, level: Level, still_write_to_file: bool) -> Self {
    //     match self.ignores.entry(target.into()) {
    //         Entry::Occupied(mut occupied_entry) => {
    //             if level < occupied_entry.get().0 {
    //                 occupied_entry.insert((level, still_write_to_file));
    //             }
    //         }
    //         Entry::Vacant(vacant_entry) => {
    //             vacant_entry.insert((level, still_write_to_file));
    //         }
    //     }
    //     self
    // }

    pub fn set_default_commands(mut self, default_commands: bool) -> Self {
        self.default_commands = default_commands;
        self
    }

    fn process_default_commands(&self) {
        let Some(cmd_name) = std::env::args().nth(1) else {
            return;
        };
        match cmd_name.as_str() {
            "clean" | "Clean" => {
                match &self.new_working_directory {
                    NewWorkingDirectory::Current => {
                        if Path::new("app.log").exists() {
                            std::fs::remove_file("app.log").expect("Failed to remove app.log");
                        }
                    }
                    NewWorkingDirectory::Custom(path) => {
                        let path = path.join("app.log");
                        if path.exists() {
                            std::fs::remove_file(path).expect("Failed to remove app.log");
                        }
                    }
                    NewWorkingDirectory::Automatic => {
                        if Path::new("output").exists() {
                            std::fs::remove_dir_all("output")
                                .expect("Failed to remove output directory");
                        }
                    }
                }
                println!("Clean Successful");
            }
            _ => return,
        }
        std::process::exit(0);
    }

    pub fn init<C: Configuration>(self) -> C {
        if self.default_commands {
            if C::is_not_default_compatible() {
                panic!(
                    "Default commands are not compatible with {}. Disable `default_commands`.",
                    type_name::<C>()
                );
            }
            self.process_default_commands();
        }

        let env_var = std::env::var(EMBEDDED_KEY);
        match env_var {
            Ok(val) => {
                if val == EMBEDDED_VAL {
                    return subprocess_fn();
                }
            }
            Err(VarError::NotPresent) => {}
            Err(e) => {
                panic!("Failed to read environment variable: {e}");
            }
        }

        if let Some(new_current_dir) = self.new_working_directory.into_path_buf() {
            std::fs::create_dir_all(&new_current_dir)
                .expect("Failed to create new working directory");
            let current_dir = std::env::current_dir()
                .expect("Failed to get current directory")
                .canonicalize()
                .expect("Failed to canonicalize current directory");

            for path_ref in self.path_reference {
                let old_path = match &path_ref {
                    PathReference::Copy(path) => path,
                    PathReference::Symlink(path) => path,
                };
                let path = old_path
                    .canonicalize()
                    .expect("Failed to canonicalize path");
                if !path.starts_with(&current_dir) {
                    panic!("File reference ({old_path:?}) must be within current directory");
                }

                let new_path = if let Some(mut parent) = path.parent() {
                    parent = parent.strip_prefix(&current_dir).unwrap();
                    let new_parent = new_current_dir.join(parent);
                    std::fs::create_dir_all(&new_parent)
                        .expect("Failed to create parent directory inside new working directory");
                    new_parent.join(path.file_name().expect("Invalid copy over path"))
                } else {
                    new_current_dir.join(path.file_name().expect("Invalid copy over path"))
                };

                if let PathReference::Copy(_) = path_ref {
                    if path.is_file() {
                        std::fs::copy(path, new_path).expect("Failed to copy file");
                    } else {
                        unimplemented!("Only files can be copied for now");
                    }
                } else {
                    #[cfg(unix)]
                    {
                        std::os::unix::fs::symlink(path, new_path)
                            .expect("Failed to create symlink");
                    }
                    #[cfg(windows)]
                    {
                        if path.is_dir() {
                            std::os::windows::fs::symlink_dir(path, new_path)
                                .expect("Failed to create symlink");
                        } else {
                            std::os::windows::fs::symlink_file(path, new_path)
                                .expect("Failed to create symlink");
                        }
                    }
                }
            }
            std::env::set_current_dir(new_current_dir).expect("Failed to set current directory");
        }

        let mut shmem = None;
        let mut flink = String::new();
        for i in 0..1024 {
            flink = format!(".lumpur-{i}.shmem");
            match ShmemConf::new().size(4096).flink(&flink).create() {
                Ok(m) => {
                    shmem = Some(m);
                    break;
                }
                Err(ShmemError::LinkExists) => {}
                Err(e) => {
                    panic!("Failed to create shared memory segment: {e}");
                }
            };
        }
        let Some(shmem) = shmem else {
            panic!("Failed to create shared memory segment. All slots occupied");
        };
        let (ctrlc_evt, _used_bytes) = unsafe {
            raw_sync::events::Event::new(shmem.as_ptr(), true)
                .expect("Failed to create ctrl-c event")
        };

        let log_file =
            std::fs::File::create("app.log").expect("Failed to create log file (app.log)");
        let mut log_file = LineWriter::new(log_file);
        writeln!(
            log_file,
            "!Program started with pid: {}",
            std::process::id()
        )
        .expect("Failed to write to log file (app.log)");
        if std::env::args().len() > 1 {
            write!(log_file, "!Arguments:").expect("Failed to write to log file (app.log)");
            for arg in std::env::args().skip(1) {
                write!(log_file, " {}", arg).expect("Failed to write to log file (app.log)");
            }
            writeln!(log_file).expect("Failed to write to log file (app.log)");
        } else {
            writeln!(log_file, "!No arguments provided")
                .expect("Failed to write to log file (app.log)");
        }
        let (write_tx, write_rx) = std::sync::mpsc::channel::<Arc<LogMessage>>();
        let total_ignores: &_ = Box::leak(Box::new(self.total_ignores));
        let console_ignores: &_ = Box::leak(Box::new(self.console_ignores));
        let write_thr = std::thread::spawn(move || log_write_thread(write_rx, log_file));

        let max_lines: usize = std::env::var("MAX_LINES")
            .map(|s| s.parse().unwrap_or(1000))
            .unwrap_or(1000);
        let mut child = Command::new(std::env::current_exe().expect("Failed to get current exe"))
            .env(EMBEDDED_KEY, EMBEDDED_VAL)
            .env(SHMEM_VAR_KEY, flink)
            .args(std::env::args().skip(1))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("Failed to spawn child process");
        let (log_tx, log_rx) = std::sync::mpsc::channel::<Arc<LogMessage>>();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let stderr = BufReader::new(child.stderr.take().unwrap());
        let current_dir: &_ = std::env::current_dir()
            .expect("Failed to get current dir")
            .canonicalize()
            .expect("Failed to canonicalize current dir")
            .leak();

        let f = make_line_f(
            log_tx.clone(),
            write_tx.clone(),
            Level::INFO,
            "stdout",
            current_dir,
            total_ignores,
            console_ignores,
        );
        std::thread::spawn(move || {
            for line in stdout.lines() {
                let Ok(line) = line else {
                    break;
                };
                f(line);
            }
        });
        let f = make_line_f(
            log_tx,
            write_tx,
            Level::ERROR,
            "stderr",
            current_dir,
            total_ignores,
            console_ignores,
        );
        std::thread::spawn(move || {
            for line in stderr.lines() {
                let Ok(line) = line else {
                    break;
                };
                f(line);
            }
        });

        let mut siv = cursive::default();
        let theme = Theme::terminal_default();
        siv.set_theme(theme);

        let mut program_info_style = Style::terminal_default();
        program_info_style.color.front = ColorType::Color(Color::Rgb(50, 200, 50));

        siv.add_fullscreen_layer(
            Layer::with_color(
                LinearLayout::vertical()
                    .child(TextView::new("       [PROGRAM STARTED]").style(program_info_style))
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

            siv.call_on_name(
                LOG_SCROLL_VIEW,
                |log_scroll_view: &mut ScrollView<LinearLayout>| {
                    if extra_info_visible {
                        log_scroll_view.set_scroll_strategy(ScrollStrategy::KeepRow);
                    } else {
                        log_scroll_view.set_scroll_strategy(ScrollStrategy::StickToBottom);
                    }
                },
            );

            siv.call_on_name(LOG_VIEW, |log_view: &mut LinearLayout| {
                for i in 0..log_view.len() {
                    if let Some(_) = log_view
                        .get_child_mut(i)
                        .unwrap()
                        .downcast_mut::<TextView>()
                    {
                        continue;
                    }
                    let line: &mut ThemedView<NamedView<LinearLayout>> =
                        log_view.get_child_mut(i).unwrap().downcast_mut().unwrap();
                    let line = &mut *line.get_inner_mut().get_mut();
                    let top: &mut LinearLayout =
                        line.get_child_mut(0).unwrap().downcast_mut().unwrap();
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

        let ctrlc_count: &_ = Box::leak(Box::new(AtomicUsize::new(0)));
        siv.menubar()
            .add_leaf(StyledString::styled("Quit (Ctrl-C)", menu_style), |_| {
                ctrlc_count.fetch_add(1, Ordering::Relaxed);
            });
        siv.set_global_callback(Event::CtrlChar('c'), move |_| {
            ctrlc_count.fetch_add(1, Ordering::Relaxed);
        });

        siv.set_autohide_menu(false);

        // We must not drop any errors past this point as the UI has spun up

        let mut siv = siv.into_runner();
        let mut last_ctrlc_count = 0;
        siv.refresh();
        let mut exit_code = 0;
        let mut line_id = 0usize;
        let mut last_message_aggregate = String::new();
        let mut last_message_count = 0usize;
        let mut child = Some(child);

        while siv.is_running() {
            siv.step();
            let mut updated = false;
            while let Ok(log) = log_rx.try_recv() {
                let current_message_aggregate = log.aggregate();

                siv.call_on_name::<LinearLayout, _, _>(LOG_VIEW, |log_view| {
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
                            let message_text: &mut TextView =
                                top.get_child_mut(2).unwrap().downcast_mut().unwrap();
                            // This will update the timestamp if needed
                            message_text.set_content(log.create_ui_message());
                            return;
                        }
                    }
                    last_message_aggregate = current_message_aggregate;
                    last_message_count = 1;

                    if log_view.len() >= max_lines {
                        log_view.remove_child(0);
                    }

                    let mut theme = Theme::terminal_default();
                    match &*log {
                        LogMessage::Stdio { level: Level::ERROR, .. } | LogMessage::Standard { level: Level::ERROR, .. } => {
                            theme.palette.set_color("Primary", Color::Rgb(240, 10, 30));
                        }
                        LogMessage::Stdio { level: Level::WARN, .. } | LogMessage::Standard { level: Level::WARN, .. } => {
                            theme.palette.set_color("Primary", Color::Rgb(200, 200, 40));
                        }
                        _ => {}
                    };
                    let line_name = line_id.to_string();
                    let line_name2 = line_name.clone();
                    let extra_info_text = match &*log {
                        LogMessage::Standard { target, filename, line_number, thread_name, .. } => {
                            format!("           target: {target}    location: {filename}:{line_number}    thread: {thread_name}  ")
                        }
                        LogMessage::Stdio { stdio, .. } => {
                            format!("           location: {stdio} (avoid using println or eprintln)")
                        }
                    };
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
                                                        line.add_child(
                                                            TextView::new(extra_info_text.clone())
                                                        );
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
                                        .child(TextView::new(log.create_ui_message()))
                                )
                                .with_name(line_name2)
                        )
                    );
                    line_id += 1;
                });
                updated = true;
            }
            if let Some(child_unwrapped) = &mut child {
                match child_unwrapped.try_wait() {
                    Ok(Some(status)) => {
                        if !status.success() {
                            exit_code = 1;
                        }
                        child = None;
                        siv.call_on_name::<LinearLayout, _, _>(LOG_VIEW, |log_view| {
                            log_view.add_child(
                                TextView::new("       [PROGRAM ENDED (Press Ctrl-C again)]")
                                    .style(program_info_style),
                            );
                        });
                        updated = true;
                    }
                    Ok(None) => {}
                    Err(e) => {
                        exit_code = 1;
                        child = None;
                        siv.call_on_name::<LinearLayout, _, _>(LOG_VIEW, |log_view| {
                            log_view.add_child(
                                TextView::new(format!("       [PROGRAM WAIT ERROR] {e}"))
                                    .style(program_info_style),
                            );
                        });
                        updated = true;
                    }
                }
            }
            let new_ctrlc_count = ctrlc_count.load(Ordering::Relaxed);
            if new_ctrlc_count != last_ctrlc_count {
                last_ctrlc_count = new_ctrlc_count;
                if let Some(child) = &mut child {
                    if new_ctrlc_count == 1 {
                        if let Err(e) = ctrlc_evt.set(EventState::Signaled) {
                            eprintln!("Failed to signal ctrl-c event: {e}");
                        }
                    } else {
                        exit_code = 1;
                        if let Err(e) = child.kill() {
                            eprintln!("Failed to kill child process: {e}");
                        } else {
                            eprintln!("Process killed");
                        }
                        siv.quit();
                    }
                } else {
                    siv.quit();
                }
            }
            if updated {
                siv.refresh();
            }
        }
        // Very important to drop to return the terminal to its original state
        drop(siv);
        // Drop these to remove the shared memory segment file
        drop(ctrlc_evt);
        drop(shmem);
        let _ = write_thr.join();
        std::process::exit(exit_code);
    }
}

pub fn init<C: Configuration>() -> C {
    LumpurBuilder::default().init()
}
