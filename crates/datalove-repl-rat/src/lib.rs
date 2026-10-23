//! The Ratatui REPL application and the terminal that runs it.

use datalove_repl as repl;
use repl::app::{Panel, ReplApp, ReplExecutor, Scroll, UiAction};

mod help;
mod render;
mod term;

pub use term::run;

use tui_textarea::TextArea;

/// The UI-agnostic app, and the text area its input is typed in.
///
/// Everything but what touches the text area is the app's, reached through
/// `repl`; what is here gives the app the text and does what it says to it.
struct RatatuiApp<E: ReplExecutor> {
    repl: ReplApp<E>,
    textarea: TextArea<'static>,
}

impl<E: ReplExecutor> RatatuiApp<E> {
    fn with_stderr_log(executor: E, stderr_log_path: std::path::PathBuf) -> Self {
        Self {
            repl: ReplApp::with_stderr_log(executor, stderr_log_path),
            textarea: TextArea::default(),
        }
    }

    fn textarea(&self) -> &TextArea<'static> {
        &self.textarea
    }

    /// What the input holds.
    fn typed(&self) -> String {
        self.textarea.lines().join("\n")
    }

    fn handle_input_key(&mut self, key: crossterm::event::KeyEvent) {
        self.textarea.input(key);
    }

    fn submit_input(&mut self) {
        let action = self.repl.submit_input(self.typed());
        self.process_ui_action(action);
    }

    fn process_ui_action(&mut self, action: UiAction) {
        match action {
            UiAction::SetMultilineInput { lines } => {
                self.textarea = TextArea::from(lines);
                self.textarea.move_cursor(tui_textarea::CursorMove::Bottom);
            }
            UiAction::SetInput { text } => {
                self.textarea = TextArea::from([text]);
                self.textarea.move_cursor(tui_textarea::CursorMove::End);
            }
            UiAction::ClearInput => {
                self.textarea = TextArea::default();
            }
            UiAction::None => {}
        }
    }

    /// Scroll whichever pane a scroll key goes to, which may be the input.
    fn handle_scroll_key(&mut self, scroll: Scroll, key: crossterm::event::KeyEvent) {
        let ctrl = key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL);
        match self.repl.scroll_target(ctrl) {
            // The plain keys of the focused code panel move between units;
            // lent from the input, or with Ctrl from the bottom, they scroll.
            Panel::Code if self.repl.focus() == Panel::Code => self.repl.move_in_code(scroll),
            Panel::Code => self.repl.scroll_code(scroll),
            Panel::Bottom => self.repl.scroll_bottom(scroll),
            Panel::Input => {
                self.textarea.input(key);
            }
        }
    }

    /// Ask to throw away a multiline input or an edit; see
    /// [`ReplApp::cancel_input`].
    fn cancel_input(&mut self) {
        let action = self.repl.cancel_input();
        self.process_ui_action(action);
    }

    /// Edit the selected unit in the input; see [`ReplApp::begin_edit`].
    fn begin_edit(&mut self) {
        let action = self.repl.begin_edit(self.typed());
        self.process_ui_action(action);
    }

    /// Write a unit to go before the selected one; see
    /// [`ReplApp::begin_insert`].
    fn begin_insert(&mut self) {
        let action = self.repl.begin_insert(self.typed());
        self.process_ui_action(action);
    }
}
