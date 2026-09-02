//! egui front end: toolbar, page surface, and input forwarding.
//!
//! The window never touches page state directly. It drains `Update`s from the
//! engine channel, renders the newest frame as a texture, and maps pointer,
//! wheel, and keyboard activity back into `Command`s.

use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

use eframe::egui;

use crate::engine::{Command, Update};

/// Repaint cadences, mirroring the engine pacing so the UI is never the
/// bottleneck nor the idler.
const REPAINT_BUSY_MS: u64 = 33;
const REPAINT_ACTIVE_MS: u64 = 60;
const REPAINT_IDLE_MS: u64 = 150;

pub struct TelemacoApp {
    engine: Sender<Command>,
    updates: Receiver<Update>,
    texture: Option<egui::TextureHandle>,
    url_field: String,
    shown_url: String,
    title: String,
    loading: bool,
    can_back: bool,
    can_forward: bool,
    error: Option<String>,
    omnibar_focused: bool,
    omnibar_id: Option<egui::Id>,
    last_frame_change: Option<Instant>,
    sent_viewport: Option<(u32, u32, f32)>,
    last_click: Option<(Instant, egui::Pos2, u32)>,
}

impl TelemacoApp {
    pub fn new(cc: &eframe::CreationContext<'_>, engine: Sender<Command>, updates: Receiver<Update>) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        Self {
            engine,
            updates,
            texture: None,
            url_field: String::new(),
            shown_url: String::new(),
            title: String::new(),
            loading: false,
            can_back: false,
            can_forward: false,
            error: None,
            omnibar_focused: false,
            omnibar_id: None,
            last_frame_change: None,
            sent_viewport: None,
            last_click: None,
        }
    }

    fn navigate(&mut self) {
        let url = self.url_field.trim().to_string();
        if url.is_empty() {
            return;
        }
        self.loading = true;
        self.error = None;
        let _ = self.engine.send(Command::Navigate { url });
    }

    fn drain_updates(&mut self, ctx: &egui::Context) {
        while let Ok(update) = self.updates.try_recv() {
            match update {
                Update::Frame { rgba, width, height } => {
                    let image =
                        egui::ColorImage::from_rgba_unmultiplied([width as usize, height as usize], &rgba);
                    if self.texture.is_none() {
                        self.texture = Some(
                            ctx.load_texture("page-frame", image, egui::TextureOptions::LINEAR),
                        );
                    } else if let Some(texture) = self.texture.as_mut() {
                        texture.set(image, egui::TextureOptions::LINEAR);
                    }
                    self.last_frame_change = Some(Instant::now());
                }
                Update::Status { url, title, loading, can_back, can_forward } => {
                    // Never clobber what the user is typing into the omnibar.
                    if url != self.shown_url && !self.omnibar_focused {
                        self.url_field = url.clone();
                    }
                    self.shown_url = url;
                    self.title = title;
                    self.loading = loading;
                    self.can_back = can_back;
                    self.can_forward = can_forward;
                    let window_title = if self.title.is_empty() {
                        "Telemaco".to_string()
                    } else {
                        format!("{} - Telemaco", self.title)
                    };
                    ctx.send_viewport_cmd(egui::ViewportCommand::Title(window_title));
                }
                Update::Error(message) => self.error = Some(message),
            }
        }
    }

    fn toolbar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("telemaco-toolbar").show(ctx, |ui| {
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                if ui.add_enabled(self.can_back, egui::Button::new("Back")).clicked() {
                    let _ = self.engine.send(Command::Back);
                }
                if ui.add_enabled(self.can_forward, egui::Button::new("Forward")).clicked() {
                    let _ = self.engine.send(Command::Forward);
                }
                if ui.button("Reload").on_hover_text("Reload page (Cmd+R)").clicked() {
                    let _ = self.engine.send(Command::Reload);
                }
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.url_field)
                        .hint_text("Search or type a URL")
                        .desired_width(ui.available_width()),
                );
                self.omnibar_focused = response.has_focus();
                self.omnibar_id = Some(response.id);
                if response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                    self.navigate();
                }
                if self.loading {
                    ui.add(egui::Spinner::new().size(16.0));
                }
            });
            if let Some(error) = self.error.clone() {
                ui.colored_label(egui::Color32::from_rgb(231, 76, 60), error);
            }
            ui.add_space(2.0);
        });
        // Focus the omnibar with Cmd+L. The raw Cmd+L key event is ignored in
        // forward_input so it never reaches the page.
        if ctx.input_mut(|input| {
            input.consume_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::L))
        }) {
            if let Some(id) = self.omnibar_id {
                ctx.memory_mut(|memory| memory.request_focus(id));
            }
        }
    }
    fn maybe_send_viewport(&mut self, ctx: &egui::Context, page_rect: egui::Rect) {
        let pps = ctx.pixels_per_point();
        // CDP width/height are CSS pixels: the page viewport must match the
        // window's logical size, and the device scale factor supplies the
        // extra pixels so captures stay crisp on retina displays.
        let width = page_rect.width().round().max(1.0) as u32;
        let height = page_rect.height().round().max(1.0) as u32;
        // Quantize the scale so tiny pps jitter does not re-send overrides.
        let scale = (pps * 2.0).round() / 2.0;
        let viewport = (width, height, scale);
        if self.sent_viewport != Some(viewport) {
            self.sent_viewport = Some(viewport);
            let _ = self.engine.send(Command::Viewport { width, height, scale });
        }
    }

    fn paint_page(&mut self, ui: &mut egui::Ui, page_rect: egui::Rect) {
        let Some(texture) = self.texture.clone() else {
            ui.painter().rect_filled(page_rect, 0.0, egui::Color32::from_rgb(24, 24, 24));
            return;
        };
        let image = egui::Image::from_texture(&texture).fit_to_exact_size(page_rect.size());
        ui.put(page_rect, image);
    }
    fn forward_input(&mut self, ctx: &egui::Context, page_rect: egui::Rect) {
        let (pointer_pos, primary_pressed, primary_released, secondary_pressed, secondary_released, scroll_delta) =
            ctx.input(|input| {
                (
                    input.pointer.interact_pos(),
                    input.pointer.primary_pressed(),
                    input.pointer.primary_released(),
                    input.pointer.secondary_pressed(),
                    input.pointer.secondary_released(),
                    input.raw_scroll_delta,
                )
            });
        let modifiers = ctx.input(|input| input.modifiers);

        if let Some(pos) = pointer_pos {
            if page_rect.contains(pos) {
                let x = pos.x - page_rect.left();
                let y = pos.y - page_rect.top();
                let mods = modifier_mask(modifiers);
                if primary_pressed || secondary_pressed {
                    let button = if primary_pressed { "left" } else { "right" };
                    let click_count = self.next_click_count(pos);
                    let _ = self.engine.send(Command::MousePressed {
                        x,
                        y,
                        button,
                        click_count,
                        modifiers: mods,
                    });
                }
                if primary_released || secondary_released {
                    let button = if primary_released { "left" } else { "right" };
                    let click_count = self.last_click.map(|(_, _, count)| count).unwrap_or(1);
                    let _ = self.engine.send(Command::MouseReleased {
                        x,
                        y,
                        button,
                        click_count,
                        modifiers: mods,
                    });
                }
                if scroll_delta != egui::Vec2::ZERO {
                    // egui's scroll delta is positive for "up"; a CDP WheelEvent
                    // with deltaY > 0 scrolls down, so negate both axes.
                    let _ = self.engine.send(Command::Wheel {
                        x,
                        y,
                        dx: -scroll_delta.x,
                        dy: -scroll_delta.y,
                    });
                }
            }
        }
        let events = ctx.input(|input| input.events.clone());
        for event in events {
            match &event {
                egui::Event::Key { key: egui::Key::R, pressed: true, modifiers: key_mods, .. }
                    if key_mods.command =>
                {
                    let _ = self.engine.send(Command::Reload);
                }
                // Cmd+L is handled by toolbar(); never forward it to the page.
                egui::Event::Key { key: egui::Key::L, pressed: true, modifiers: key_mods, .. }
                    if key_mods.command => {}
                other if !self.omnibar_focused => forward_page_event(&self.engine, other, modifiers),
                _ => {}
            }
        }
    }

    fn next_click_count(&mut self, pos: egui::Pos2) -> u32 {
        let now = Instant::now();
        let count = match &self.last_click {
            Some((when, last_pos, count))
                if now.duration_since(*when) < Duration::from_millis(350)
                    && (*last_pos - pos).length() < 6.0 =>
            {
                count + 1
            }
            _ => 1,
        };
        self.last_click = Some((now, pos, count));
        count
    }

    fn schedule_repaint(&self, ctx: &egui::Context) {
        let wait = if self.loading {
            REPAINT_BUSY_MS
        } else if self
            .last_frame_change
            .is_some_and(|change| change.elapsed() < Duration::from_millis(400))
        {
            REPAINT_ACTIVE_MS
        } else {
            REPAINT_IDLE_MS
        };
        ctx.request_repaint_after(Duration::from_millis(wait));
    }
}

impl eframe::App for TelemacoApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_updates(ctx);
        self.toolbar(ctx);
        let page_rect = ctx.available_rect();
        self.maybe_send_viewport(ctx, page_rect);
        egui::CentralPanel::default()
            .frame(egui::Frame::none())
            .show(ctx, |ui| self.paint_page(ui, page_rect));
        self.forward_input(ctx, page_rect);
        self.schedule_repaint(ctx);
    }
}

fn forward_page_event(
    engine: &Sender<Command>,
    event: &egui::Event,
    input_modifiers: egui::Modifiers,
) {
    match event {
        egui::Event::Text(text) | egui::Event::Paste(text) => {
            let _ = engine.send(Command::Char {
                text: text.clone(),
                modifiers: modifier_mask(input_modifiers),
            });
        }
        egui::Event::Key { key, pressed, modifiers: key_mods, .. } => {
            let Some((key_name, code)) = cdp_key_name(*key) else {
                return;
            };
            let mods = modifier_mask(*key_mods);
            if *pressed {
                let _ = engine.send(Command::KeyDown {
                    key: key_name.to_string(),
                    code: code.to_string(),
                    modifiers: mods,
                });
                let _ = engine.send(Command::KeyUp {
                    key: key_name.to_string(),
                    code: code.to_string(),
                    modifiers: mods,
                });
            }
        }
        _ => {}
    }
}

/// egui -> CDP modifier bitmask (Alt=1, Ctrl=2, Meta=4, Shift=8), the same
/// encoding Input.dispatchMouseEvent reads.
fn modifier_mask(modifiers: egui::Modifiers) -> u32 {
    let mut mask = 0;
    if modifiers.alt {
        mask |= 1;
    }
    if modifiers.ctrl {
        mask |= 2;
    }
    if modifiers.command || modifiers.mac_cmd {
        mask |= 4;
    }
    if modifiers.shift {
        mask |= 8;
    }
    mask
}

/// Map an egui key to the CDP key/code pair. Printable characters (including
/// space) arrive as Text/Paste events instead, so they are deliberately absent.
fn cdp_key_name(key: egui::Key) -> Option<(&'static str, &'static str)> {
    use egui::Key;
    let name: &'static str = match key {
        Key::Enter => "Enter",
        Key::Backspace => "Backspace",
        Key::Tab => "Tab",
        Key::Escape => "Escape",
        Key::ArrowDown => "ArrowDown",
        Key::ArrowUp => "ArrowUp",
        Key::ArrowLeft => "ArrowLeft",
        Key::ArrowRight => "ArrowRight",
        Key::Home => "Home",
        Key::End => "End",
        Key::PageUp => "PageUp",
        Key::PageDown => "PageDown",
        Key::Delete => "Delete",
        Key::Insert => "Insert",
        Key::F1 => "F1",
        Key::F2 => "F2",
        Key::F3 => "F3",
        Key::F4 => "F4",
        Key::F5 => "F5",
        Key::F6 => "F6",
        Key::F7 => "F7",
        Key::F8 => "F8",
        Key::F9 => "F9",
        Key::F10 => "F10",
        Key::F11 => "F11",
        Key::F12 => "F12",
        _ => return None,
    };
    Some((name, name))
}