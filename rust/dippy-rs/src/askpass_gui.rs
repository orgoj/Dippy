//! The `dippy-askpass` window (eframe/egui). Logic lives in [`crate::askpass`].
//!
//! The window comes to the front without taking keyboard focus: winit's
//! `with_active(false)` on Windows and macOS; on X11 (where winit ignores it)
//! `WM_HINTS` input=False, `_NET_WM_USER_TIME=0` and `_NET_WM_STATE_ABOVE`
//! are set before mapping, and a click sets input=True and asks for focus.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use eframe::egui::{self, Key, Modifiers, ViewportCommand};

use crate::askpass::{self, ACTIONS, Arming, Check, Files, KINDS, Request};

#[cfg(all(unix, not(target_os = "macos")))]
mod x11 {
    use x11rb::connection::Connection;
    use x11rb::properties::WmHints;
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt, PropMode};
    use x11rb::rust_connection::RustConnection;
    use x11rb::wrapper::ConnectionExt as _;

    pub struct Window {
        conn: RustConnection,
        id: u32,
    }

    fn err(e: impl std::fmt::Display) -> String {
        e.to_string()
    }

    impl Window {
        /// Hints for an unmapped window: no focus on map, stay above.
        pub fn prepare(id: u32) -> Result<Self, String> {
            let (conn, _) = x11rb::connect(None).map_err(err)?;
            let w = Self { conn, id };
            w.set_input(false)?;
            let atom = |name: &[u8]| -> Result<u32, String> {
                Ok(w.conn
                    .intern_atom(false, name)
                    .map_err(err)?
                    .reply()
                    .map_err(err)?
                    .atom)
            };
            let (user_time, state, above) = (
                atom(b"_NET_WM_USER_TIME")?,
                atom(b"_NET_WM_STATE")?,
                atom(b"_NET_WM_STATE_ABOVE")?,
            );
            w.conn
                .change_property32(PropMode::REPLACE, id, user_time, AtomEnum::CARDINAL, &[0])
                .map_err(err)?;
            w.conn
                .change_property32(PropMode::REPLACE, id, state, AtomEnum::ATOM, &[above])
                .map_err(err)?;
            w.conn.flush().map_err(err)?;
            Ok(w)
        }

        /// ICCCM input hint: whether the window manager may give it focus.
        pub fn set_input(&self, input: bool) -> Result<(), String> {
            let mut hints = WmHints::get(&self.conn, self.id)
                .map_err(err)?
                .reply()
                .map_err(err)?
                .unwrap_or_default();
            hints.input = Some(input);
            hints.set(&self.conn, self.id).map_err(err)?;
            self.conn.flush().map_err(err)
        }
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn prepare_window(cc: &eframe::CreationContext) -> Option<x11::Window> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let id = match cc.window_handle().ok()?.as_raw() {
        RawWindowHandle::Xlib(h) => h.window as u32,
        RawWindowHandle::Xcb(h) => h.window.get(),
        _ => return None,
    };
    x11::Window::prepare(id).ok()
}

#[cfg(not(all(unix, not(target_os = "macos"))))]
fn prepare_window(_cc: &eframe::CreationContext) -> Option<()> {
    None
}

/// Decision and note, written by the window, read after it closes.
type Outcome = Rc<RefCell<(Option<&'static str>, String)>>;

struct Dialog {
    req: Request,
    started: Instant,
    shown: bool,
    #[cfg(all(unix, not(target_os = "macos")))]
    x11: Option<x11::Window>,
    arming: Arming,
    was_focused: bool,
    action: &'static str,
    kind: &'static str,
    /// Comma-separated context flags of the rule.
    tags: String,
    pattern: String,
    project: bool,
    home: PathBuf,
    user_path: PathBuf,
    project_path: PathBuf,
    /// Index of the cause the rule is for.
    selected: usize,
    /// Current decision of each cause; refreshed after saving.
    statuses: Result<Vec<&'static str>, String>,
    /// (selected, action, kind, tags, pattern, project) of `check`.
    checked: Option<(usize, &'static str, &'static str, String, String, bool)>,
    check: Option<Check>,
    saved: Option<Result<PathBuf, String>>,
    outcome: Outcome,
}

impl Dialog {
    fn target(&self) -> &PathBuf {
        if self.project {
            &self.project_path
        } else {
            &self.user_path
        }
    }

    fn refresh_check(&mut self) {
        let key = (
            self.selected,
            self.action,
            self.kind,
            self.tags.clone(),
            self.pattern.clone(),
            self.project,
        );
        if self.checked.as_ref() != Some(&key) {
            let files = Files {
                user: &self.user_path,
                target: self.target(),
            };
            self.check = Some(askpass::check(
                &self.req,
                self.req.causes.get(self.selected),
                self.action,
                self.kind,
                &self.tags,
                &self.pattern,
                &files,
            ));
            self.checked = Some(key);
        }
    }

    fn finish(&self, ctx: &egui::Context, decision: &'static str) {
        self.outcome.borrow_mut().0 = Some(decision);
        ctx.send_viewport_cmd(ViewportCommand::Close);
    }

    fn save(&mut self) {
        let Some(check) = &self.check else { return };
        let target = self.target().clone();
        let date = chrono::Local::now().format("%Y-%m-%d").to_string();
        self.saved = Some(
            crate::admin::add_rule(&target, &check.line, &date)
                .map(|()| target.clone())
                .map_err(|e| format!("{}: {e}", target.display())),
        );
        self.checked = None;
        self.statuses = askpass::statuses(&self.req, &self.user_path);
    }

    /// Focus handling and arming; returns whether approval is armed.
    fn track_focus(&mut self, ctx: &egui::Context) -> bool {
        let now = Instant::now();
        let focused = ctx.input(|i| i.focused);
        if focused != self.was_focused {
            self.arming.focus(focused, now);
            self.was_focused = focused;
        }
        if !focused && ctx.input(|i| i.pointer.any_pressed()) {
            #[cfg(all(unix, not(target_os = "macos")))]
            if let Some(w) = &self.x11 {
                let _ = w.set_input(true);
            }
            ctx.send_viewport_cmd(ViewportCommand::Focus);
        }
        let armed = self.arming.armed(now);
        if !armed {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
        armed
    }
}

const LABEL_WIDTH: f32 = 80.0;

fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(10.0);
    ui.label(egui::RichText::new(title).strong().size(13.0));
    ui.separator();
}

/// A labelled row whose content wraps in the remaining width.
fn field(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal_top(|ui| {
        ui.allocate_ui(egui::vec2(LABEL_WIDTH, 0.0), |ui| {
            ui.set_min_width(LABEL_WIDTH);
            ui.strong(label);
        });
        ui.vertical(add);
    });
}

impl Dialog {
    fn header(&self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.strong(self.req.tool.as_deref().unwrap_or("dippy"));
            ui.label("in");
            ui.monospace(askpass::tilde(&self.req.cwd.to_string_lossy(), &self.home));
        });
        ui.add_space(2.0);
    }

    fn body(&mut self, ui: &mut egui::Ui, armed: bool) {
        let home = self.home.clone();
        section(ui, "REQUEST");
        let mut subject = self.req.subject();
        ui.add(
            egui::TextEdit::multiline(&mut subject)
                .desired_rows(2)
                .desired_width(f32::INFINITY)
                .code_editor(),
        );

        section(ui, "WHY IT ASKED");
        if !self.req.message.is_empty() {
            ui.label(&self.req.message);
        }
        if self.req.causes.len() > 1 {
            ui.weak("Select a part to write a rule for it; save one rule per part.");
        }
        let mut select = None;
        for (i, c) in self.req.causes.iter().enumerate() {
            ui.horizontal_wrapped(|ui| {
                if ui.radio(self.selected == i, c.kind.as_str()).clicked() {
                    select = Some(i);
                }
                ui.monospace(&c.target);
                match self.statuses.as_ref().map(|s| s.get(i)) {
                    Ok(Some(&"ask")) => ui.weak("→ asks"),
                    Ok(Some(d)) => ui.strong(format!("→ now {d}")),
                    _ => ui.weak(""),
                };
            });
            let rule = c.rule.as_deref().map_or_else(
                || "built-in (no rule matched)".to_string(),
                |r| format!("rule  {}", askpass::tilde(r, &home)),
            );
            ui.indent(("cause", i), |ui| {
                ui.weak(rule);
                if !c.flags.is_empty() {
                    ui.weak(format!("context  {}", c.flags.join(", ")));
                }
            });
        }
        if let Err(e) = &self.statuses {
            ui.colored_label(ui.visuals().error_fg_color, e);
        }
        if let Some(i) = select {
            let c = &self.req.causes[i];
            self.selected = i;
            self.kind = kind_of(&c.kind);
            self.tags = c.tags();
            self.pattern = c.target.clone();
        }

        section(ui, "ADD A RULE (optional)");
        field(ui, "Action", |ui| {
            ui.horizontal_wrapped(|ui| {
                for a in ACTIONS {
                    ui.radio_value(&mut self.action, a, a);
                }
            });
        });
        field(ui, "Kind", |ui| {
            ui.horizontal_wrapped(|ui| {
                for k in KINDS {
                    ui.radio_value(&mut self.kind, k, k);
                }
            });
        });
        field(ui, "Tags", |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.tags)
                    .hint_text("none: the rule applies in every context")
                    .desired_width(f32::INFINITY)
                    .code_editor(),
            );
        });
        field(ui, "Pattern", |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.pattern)
                    .desired_width(f32::INFINITY)
                    .code_editor(),
            );
        });
        if let Some(check) = &self.check {
            let action = self.action;
            let part = if self.req.causes.is_empty() {
                "this request"
            } else {
                "selected part"
            };
            field(ui, "Result", |ui| {
                ui.monospace(&check.line);
                let visuals = ui.visuals().clone();
                match &check.outcome {
                    Ok(o) => {
                        if check.can_save(action) {
                            ui.label(format!("✓ {part} would get: {}", o.cause));
                        } else {
                            ui.colored_label(
                                visuals.warn_fg_color,
                                format!("✗ {part} would get: {} (not {action})", o.cause),
                            );
                        }
                        if !self.req.causes.is_empty() {
                            let more = if o.request == "ask" && o.cause != "ask" {
                                " (another part still asks)"
                            } else {
                                ""
                            };
                            ui.label(format!("whole request would get: {}{more}", o.request));
                        }
                    }
                    Err(e) => {
                        ui.colored_label(visuals.error_fg_color, e);
                    }
                }
                if check.broad {
                    ui.colored_label(
                        visuals.warn_fg_color,
                        "Broad pattern: it matches far more than this request.",
                    );
                }
            });
        }
        let project = askpass::tilde(&self.project_path.to_string_lossy(), &home);
        let user = askpass::tilde(&self.user_path.to_string_lossy(), &home);
        field(ui, "Save to", |ui| {
            ui.radio_value(&mut self.project, true, format!("project   {project}"));
            ui.radio_value(&mut self.project, false, format!("user   {user}"));
        });
        let can_save = armed && self.check.as_ref().is_some_and(|c| c.can_save(self.action));
        let mut save = false;
        field(ui, "", |ui| {
            save = ui
                .add_enabled(can_save, egui::Button::new("Save rule"))
                .clicked();
            match &self.saved {
                Some(Ok(path)) => {
                    ui.label(format!(
                        "saved to {}",
                        askpass::tilde(&path.to_string_lossy(), &home)
                    ));
                }
                Some(Err(e)) => {
                    ui.colored_label(ui.visuals().error_fg_color, e);
                }
                None => {}
            }
        });
        if save {
            self.save();
        }
        ui.add_space(8.0);
    }

    /// Note, arming hint, countdown and the decision buttons.
    fn decision(
        &mut self,
        ui: &mut egui::Ui,
        armed: bool,
        left: Option<u64>,
    ) -> Option<&'static str> {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.strong("Note");
            ui.add(
                egui::TextEdit::singleline(&mut self.outcome.borrow_mut().1)
                    .desired_width(f32::INFINITY),
            );
        });
        ui.horizontal(|ui| {
            if !armed {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    "⏳ Click into this window to enable the buttons",
                );
            }
            if let Some(left) = left {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(format!("denies in {left} s"));
                });
            }
        });
        let (deny, approve) = ui.columns(2, |cols| {
            let button = |ui: &mut egui::Ui, text: &str| {
                let size = egui::vec2(ui.available_width(), 36.0);
                ui.add_enabled(
                    armed,
                    egui::Button::new(egui::RichText::new(text).size(16.0)).min_size(size),
                )
                .clicked()
            };
            (
                button(&mut cols[0], "Deny  (Esc)"),
                button(&mut cols[1], "Approve  (Ctrl+Enter)"),
            )
        });
        ui.add_space(6.0);
        if approve {
            Some("allow")
        } else if deny {
            Some("deny")
        } else {
            None
        }
    }
}

impl eframe::App for Dialog {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if !self.shown {
            self.shown = true;
            ctx.send_viewport_cmd(ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(ViewportCommand::RequestUserAttention(
                egui::UserAttentionType::Informational,
            ));
        }
        let armed = self.track_focus(&ctx);
        let left = askpass::remaining(self.req.timeout, self.started.elapsed());
        if left == Some(0) {
            self.finish(&ctx, "deny");
        }
        ctx.request_repaint_after(Duration::from_millis(250));
        self.refresh_check();

        egui::Panel::top("header").show(ui, |ui| self.header(ui));
        let clicked = egui::Panel::bottom("decision")
            .show(ui, |ui| self.decision(ui, armed, left))
            .inner;
        if let Some(d) = clicked {
            self.finish(&ctx, d);
        }
        egui::CentralPanel::default_margins().show(ui, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| self.body(ui, armed));
        });

        let (ctrl_enter, escape) = ctx.input_mut(|i| {
            (
                armed && i.consume_key(Modifiers::COMMAND, Key::Enter),
                armed && i.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        if let Some(d) = askpass::key_decision(armed, ctrl_enter, escape) {
            self.finish(&ctx, d);
        }
    }
}

/// The offered rule kind for a cause kind.
fn kind_of(kind: &str) -> &'static str {
    KINDS
        .iter()
        .find(|k| **k == kind)
        .copied()
        .unwrap_or("command")
}

/// Show the dialog; `None` when the window was closed without a decision.
fn show(req: Request) -> Result<(Option<&'static str>, String), String> {
    let outcome: Outcome = Rc::new(RefCell::new((None, String::new())));
    let shared = outcome.clone();
    let (kind, pattern) = req.prefill();
    let tags = req.prefill_tags();
    let kind = kind_of(kind);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Dippy approval")
            .with_inner_size([760.0, 640.0])
            .with_visible(false)
            .with_active(false)
            .with_window_level(egui::WindowLevel::AlwaysOnTop),
        ..Default::default()
    };
    eframe::run_native(
        "dippy-askpass",
        options,
        Box::new(move |cc| {
            let window = prepare_window(cc);
            #[cfg(not(all(unix, not(target_os = "macos"))))]
            let _ = window;
            // X11 reports no system theme; egui would fall back to dark.
            cc.egui_ctx
                .options_mut(|o| o.fallback_theme = egui::Theme::Light);
            cc.egui_ctx.set_zoom_factor(1.15);
            let user_path = crate::config::user_config_path();
            Ok(Box::new(Dialog {
                home: crate::paths::home(),
                selected: 0,
                statuses: askpass::statuses(&req, &user_path),
                user_path,
                project_path: crate::admin::project_rule_path(&req.cwd),
                req,
                started: Instant::now(),
                shown: false,
                #[cfg(all(unix, not(target_os = "macos")))]
                x11: window,
                arming: Arming::default(),
                was_focused: false,
                action: "allow",
                kind,
                tags,
                pattern,
                project: true,
                checked: None,
                check: None,
                saved: None,
                outcome: shared,
            }))
        }),
    )
    .map_err(|e| e.to_string())?;
    let result = outcome.borrow().clone();
    Ok(result)
}

/// `dippy-askpass` entry point: prints the response, returns the exit code.
pub fn main() -> i32 {
    let mut stdin = String::new();
    let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut stdin);
    let req = Request::parse(&stdin, |k| std::env::var(k).ok());
    let (out, code) = match show(req) {
        Ok((decision, note)) => askpass::response(decision.unwrap_or("deny"), &note),
        Err(e) => askpass::response("deny", &format!("GUI error: {e}")),
    };
    println!("{out}");
    code
}
