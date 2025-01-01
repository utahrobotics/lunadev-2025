use std::env::VarError;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;

use anyhow::Context;
use cursive::style::{Color, PaletteColor};
use cursive::theme::{ColorStyle, ColorType, Style, StyleType, Theme};
use cursive::view::{Nameable, Resizable, Scrollable};
use cursive::views::{Layer, LinearLayout, TextView};
use cursive::Cursive;
use tracing_log::LogTracer;

#[derive(Clone, Copy)]
enum LineType {
    Error,
    Info,
    Warn,
}

const EMBEDDED_KEY: &str = "__LUMPUR_EMBEDDED";
const EMBEDDED_VAL: &str = "1";
const LOG_VIEW: &str = "log_view";

pub fn init() -> anyhow::Result<()> {
    let env_var = std::env::var(EMBEDDED_KEY);
    match env_var {
        Ok(val) => {
            if val == EMBEDDED_VAL {
                LogTracer::init()?;
                // TODO: Init tracing subscriber
                return Ok(());
            }
        }
        Err(VarError::NotPresent) => {}
        Err(e) => return Err(e.into()),
    }

    let mut child = Command::new(std::env::current_exe()?)
        .env(EMBEDDED_KEY, EMBEDDED_VAL)
        .args(std::env::args().skip(1))
        .stdin(Stdio::inherit())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let id = child.id();
    let (lines_tx, lines_rx) = std::sync::mpsc::channel::<(String, LineType)>();
    let stdout = BufReader::new(child.stdout.take().unwrap());
    let stderr = BufReader::new(child.stderr.take().unwrap());

    let make_line_f = |tx: Sender<(String, LineType)>, prefix, ty| {
        move |line: String| {
            if line.starts_with("[ERROR ") {
                let _ = tx.send((line, LineType::Error));
            } else if line.starts_with("[INFO ") {
                let _ = tx.send((line, LineType::Info));
            } else if line.starts_with("[WARN ") {
                let _ = tx.send((line, LineType::Warn));
            } else {
                let _ = tx.send((format!("[{}] {}", prefix, line), ty));
            }
        }
    };
    let f = make_line_f(lines_tx.clone(), "INFO*", LineType::Info);
    std::thread::spawn(move || {
        for line in stdout.lines() {
            let Ok(line) = line else {
                break;
            };
            f(line);
        }
    });
    let f = make_line_f(lines_tx, "ERROR*", LineType::Error);
    std::thread::spawn(move || {
        for line in stderr.lines() {
            let Ok(line) = line else {
                break;
            };
            f(line);
        }
    });

    let mut called = false;
    ctrlc::set_handler(move || {
        if called {
            std::thread::spawn(move || {
                let _ = Command::new("kill")
                    .args(["-s", "KILL", &id.to_string()])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::inherit())
                    .output();
                std::process::exit(1);
            });
        } else {
            called = true;
            std::thread::spawn(move || {
                let result = Command::new("kill")
                    .args(["-s", "INT", &id.to_string()])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::inherit())
                    .output();
                match result {
                    Ok(output) => {
                        if output.status.success() {
                            std::process::exit(0);
                        } else {
                            eprintln!("Prcoess did not exit successfully");
                        }
                    }
                    Err(e) => {
                        eprintln!("Failed to send SIGINT to child process: {e}");
                    }
                }
                let _ = Command::new("kill")
                    .args(["-s", "KILL", &id.to_string()])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::inherit())
                    .output();
                std::process::exit(1);
            });
        }
    })
    .context("Setting Ctrl-C Handler")?;

    let mut siv = cursive::default();
    let theme = custom_theme_from_cursive(&siv);
    siv.set_theme(theme);

    let style = Style {
        effects: Default::default(),
        color: ColorStyle::terminal_default(),
    };

    let mut first_line_style = style;
    first_line_style.color.front = ColorType::Color(Color::Rgb(50, 220, 50));

    siv.add_fullscreen_layer(Layer::with_color(
        LinearLayout::vertical()
            .child(TextView::new("[PROGRAM STARTED]").style(first_line_style))
            .with_name(LOG_VIEW)
            .scrollable(),
            ColorStyle::terminal_default()
    ).full_width());
    let mut siv = siv.into_runner();

    siv.refresh();
    let mut exit_code = 0;
    while siv.is_running() {
        siv.step();
        let mut updated = false;
        while let Ok((line, ty)) = lines_rx.try_recv() {
            siv.call_on_name::<LinearLayout, _, _>(LOG_VIEW, |log_view| {
                let mut style = style;
                match ty {
                    LineType::Error => {
                        style.color.front = ColorType::Color(Color::Rgb(240, 20, 40))
                    }
                    LineType::Info => style.color.front = ColorType::Color(Color::TerminalDefault),
                    LineType::Warn => {
                        style.color.front = ColorType::Color(Color::Rgb(200, 200, 40))
                    }
                };
                log_view.add_child(TextView::new(line).style(style));
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
    }
    // Very important to drop to return the terminal to its original state
    drop(siv);
    std::process::exit(exit_code);
}

fn custom_theme_from_cursive(siv: &Cursive) -> Theme {
    let mut theme = siv.current_theme().clone();
    theme.palette[PaletteColor::Background] = Color::TerminalDefault;
    theme
}
