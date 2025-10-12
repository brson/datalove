//! Egui bindings for the datalove REPL using egui_ratatui.

use eframe::egui;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsCast;
use egui_ratatui::RataguiBackend;
use ratatui::Terminal;
use soft_ratatui::embedded_graphics_unicodefonts::{
    mono_8x13_atlas, mono_8x13_bold_atlas, mono_8x13_italic_atlas,
};
use soft_ratatui::{EmbeddedGraphics, SoftBackend};

/// Main entry point for native and WASM.
#[cfg(not(target_arch = "wasm32"))]
pub fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 600.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Datalove REPL",
        options,
        Box::new(|_cc| Ok(Box::new(ReplApp::new()))),
    )
}

/// Entry point for WASM.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn start() -> Result<(), wasm_bindgen::JsValue> {
    // Redirect panic messages to console.error
    console_error_panic_hook::set_once();

    let web_options = eframe::WebOptions::default();

    wasm_bindgen_futures::spawn_local(async {
        let document = web_sys::window()
            .expect("no window")
            .document()
            .expect("no document");
        let canvas = document
            .get_element_by_id("the_canvas_id")
            .expect("no canvas element")
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("element is not a canvas");

        eframe::WebRunner::new()
            .start(
                canvas,
                web_options,
                Box::new(|_cc| Ok(Box::new(ReplApp::new()))),
            )
            .await
            .expect("failed to start eframe");
    });

    Ok(())
}

/// The eframe application.
struct ReplApp {
    terminal: Terminal<RataguiBackend<EmbeddedGraphics>>,
    app: datalove_repl_rat::App,
}

impl ReplApp {
    fn new() -> Self {
        // Create the soft backend with embedded graphics fonts.
        let font_regular = mono_8x13_atlas();
        let font_bold = Some(mono_8x13_bold_atlas());
        let font_italic = Some(mono_8x13_italic_atlas());

        let soft_backend = SoftBackend::<EmbeddedGraphics>::new(
            100,
            50,
            font_regular,
            font_bold,
            font_italic,
        );

        let backend = RataguiBackend::new("datalove-repl", soft_backend);
        let terminal = Terminal::new(backend).unwrap();

        Self {
            terminal,
            app: datalove_repl_rat::App::new(),
        }
    }
}

impl eframe::App for ReplApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            // Handle keyboard input.
            ctx.input(|i| {
                for event in &i.events {
                    if let egui::Event::Key { key, pressed: true, modifiers: _, .. } = event {
                        match key {
                            egui::Key::Enter => self.app.submit_input(),
                            egui::Key::Backspace => self.app.delete_char(),
                            egui::Key::ArrowLeft => self.app.move_cursor_left(),
                            egui::Key::ArrowRight => self.app.move_cursor_right(),
                            egui::Key::Escape => {
                                if self.app.menu_is_open() {
                                    self.app.close_menu();
                                } else {
                                    self.app.open_menu();
                                }
                            }
                            egui::Key::ArrowUp if self.app.menu_is_open() => self.app.menu_up(),
                            egui::Key::ArrowDown if self.app.menu_is_open() => self.app.menu_down(),
                            _ => {}
                        }
                    } else if let egui::Event::Text(text) = event {
                        // Handle text input.
                        for c in text.chars() {
                            self.app.enter_char(c);
                        }
                    }
                }
            });

            // Draw the ratatui terminal.
            self.terminal
                .draw(|f| datalove_repl_rat::ui(f, &self.app))
                .expect("failed to draw terminal");

            // Render the terminal widget.
            ui.add(self.terminal.backend_mut());

            // Request repaint for interactivity.
            ctx.request_repaint();
        });
    }
}
