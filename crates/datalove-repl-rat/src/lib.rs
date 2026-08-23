//! The Ratatui REPL application and the terminal that runs it.


use rmx::prelude::*;

use datalove_repl as repl;
pub use repl::app::{ReplApp, ReplExecutor, UiAction, HistoryEntry, EntryStatus};
pub use repl::ThreadedExecutor;

mod render;
mod term;

use tui_textarea::TextArea;

/// Ratatui-specific REPL application wrapper.
pub struct RatatuiApp<E: ReplExecutor> {
    /// Core UI-agnostic REPL logic.
    pub repl: ReplApp<E>,
    /// Ratatui text area widget.
    textarea: TextArea<'static>,
}

impl<E: ReplExecutor> RatatuiApp<E> {
    /// Create a Ratatui app around an executor.
    pub fn with_executor(executor: E) -> Self {
        Self {
            repl: ReplApp::with_executor(executor),
            textarea: TextArea::default(),
        }
    }

    pub fn with_stderr_log(executor: E, stderr_log_path: std::path::PathBuf) -> Self {
        Self {
            repl: ReplApp::with_stderr_log(executor, stderr_log_path),
            textarea: TextArea::default(),
        }
    }

    /// Get reference to the textarea for rendering.
    pub fn textarea(&self) -> &TextArea<'static> {
        &self.textarea
    }

    pub fn handle_input_key(&mut self, key: crossterm::event::KeyEvent) {
        self.textarea.input(key);
    }

    pub fn delete_char(&mut self) {
        self.textarea.delete_char();
    }

    pub fn move_cursor_left(&mut self) {
        self.textarea.move_cursor(tui_textarea::CursorMove::Back);
    }

    pub fn move_cursor_right(&mut self) {
        self.textarea.move_cursor(tui_textarea::CursorMove::Forward);
    }

    pub fn enter_char(&mut self, c: char) {
        self.textarea.insert_char(c);
    }

    /// Submit the current input.
    pub fn submit_input(&mut self) {
        let input_text = self.textarea.lines().join("\n");
        let action = self.repl.submit_input(input_text);
        self.process_ui_action(action);
    }

    /// Poll for executor results and process UI actions.
    pub fn poll_results(&mut self) {
        for action in self.repl.poll_results() {
            self.process_ui_action(action);
        }
    }

    fn process_ui_action(&mut self, action: UiAction) {
        match action {
            UiAction::SetMultilineInput { lines } => {
                self.textarea = TextArea::from(lines);
                self.textarea.move_cursor(tui_textarea::CursorMove::Bottom);
            }
            UiAction::ClearInput => {
                self.textarea = TextArea::default();
            }
            UiAction::None => {}
        }
    }

    // Delegated methods.
    pub fn open_menu(&mut self) {
        self.repl.open_menu();
    }

    pub fn close_menu(&mut self) {
        self.repl.close_menu();
    }

    pub fn menu_up(&mut self) {
        self.repl.menu_up();
    }

    pub fn menu_down(&mut self) {
        self.repl.menu_down();
    }

    pub fn execute_menu_action(&mut self) {
        self.repl.execute_menu_action();
    }

    pub fn dismiss_crash_modal(&mut self) {
        self.repl.dismiss_crash_modal();
    }

    pub fn set_should_exit(&mut self, val: bool) {
        self.repl.set_should_exit(val);
    }

    pub fn menu_is_open(&self) -> bool {
        self.repl.menu_is_open()
    }

    pub fn multiline_mode(&self) -> bool {
        self.repl.multiline_mode()
    }

    pub fn should_exit(&self) -> bool {
        self.repl.should_exit()
    }

    pub fn crash_modal_is_open(&self) -> bool {
        self.repl.crash_modal_message().is_some()
    }
}

/// Re-export the UI rendering function from the render module.
pub use render::ui;

/// Re-export the terminal entry point.
pub use term::run;
