//! Web frontend for the Datalove REPL.

use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
use datalove_repl::{app::ReplApp, BlockingExecutor};

/// WASM wrapper for ReplApp.
///
/// Exposes ReplApp methods to JavaScript via wasm-bindgen.
#[wasm_bindgen]
pub struct WebReplApp {
    #[cfg(target_arch = "wasm32")]
    inner: ReplApp<BlockingExecutor>,
}

#[wasm_bindgen]
impl WebReplApp {
    /// Create a new web REPL app.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<WebReplApp, JsValue> {
        #[cfg(target_arch = "wasm32")]
        {
            console_error_panic_hook::set_once();
            Ok(WebReplApp {
                inner: ReplApp::new(),
            })
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            Err(JsValue::from_str("WebReplApp only works on wasm32 target"))
        }
    }

    /// Submit input text for processing.
    ///
    /// Returns UiAction as JsValue.
    pub fn submit_input(&mut self, input_text: String) -> JsValue {
        #[cfg(target_arch = "wasm32")]
        {
            let action = self.inner.submit_input(input_text);
            serde_wasm_bindgen::to_value(&action).unwrap()
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            JsValue::NULL
        }
    }

    /// Poll for results from executor.
    ///
    /// Returns Vec<UiAction> as JsValue.
    pub fn poll_results(&mut self) -> JsValue {
        #[cfg(target_arch = "wasm32")]
        {
            let actions = self.inner.poll_results();
            serde_wasm_bindgen::to_value(&actions).unwrap()
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            JsValue::NULL
        }
    }

    /// Get the history entries.
    ///
    /// Returns Vec<HistoryEntry> as JsValue.
    pub fn get_history(&self) -> JsValue {
        #[cfg(target_arch = "wasm32")]
        {
            serde_wasm_bindgen::to_value(&self.inner.history()).unwrap()
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            JsValue::NULL
        }
    }

    /// Get the environment variables.
    ///
    /// Returns Vec<(String, String, String)> as JsValue.
    pub fn get_environment(&self) -> JsValue {
        #[cfg(target_arch = "wasm32")]
        {
            serde_wasm_bindgen::to_value(&self.inner.environment()).unwrap()
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            JsValue::NULL
        }
    }

    /// Open the ESC menu.
    pub fn open_menu(&mut self) {
        #[cfg(target_arch = "wasm32")]
        {
            self.inner.open_menu();
        }
    }

    /// Close the ESC menu.
    pub fn close_menu(&mut self) {
        #[cfg(target_arch = "wasm32")]
        {
            self.inner.close_menu();
        }
    }

    /// Resume from menu (close it).
    pub fn resume_from_menu(&mut self) {
        #[cfg(target_arch = "wasm32")]
        {
            self.inner.close_menu();
        }
    }

    /// Exit from menu.
    pub fn exit_from_menu(&mut self) {
        #[cfg(target_arch = "wasm32")]
        {
            self.inner.set_should_exit(true);
            self.inner.close_menu();
        }
    }

    /// Check if menu is open.
    pub fn menu_is_open(&self) -> bool {
        #[cfg(target_arch = "wasm32")]
        {
            self.inner.menu_is_open()
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            false
        }
    }

    /// Get menu selection index.
    pub fn menu_selection(&self) -> usize {
        #[cfg(target_arch = "wasm32")]
        {
            self.inner.menu_selection()
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            0
        }
    }

    /// Move menu selection up.
    pub fn menu_up(&mut self) {
        #[cfg(target_arch = "wasm32")]
        {
            self.inner.menu_up();
        }
    }

    /// Move menu selection down.
    pub fn menu_down(&mut self) {
        #[cfg(target_arch = "wasm32")]
        {
            self.inner.menu_down();
        }
    }

    /// Execute selected menu action.
    pub fn execute_menu_action(&mut self) {
        #[cfg(target_arch = "wasm32")]
        {
            self.inner.execute_menu_action();
        }
    }

    /// Dismiss crash modal.
    pub fn dismiss_crash_modal(&mut self) {
        #[cfg(target_arch = "wasm32")]
        {
            self.inner.dismiss_crash_modal();
        }
    }

    /// Check if crash modal is open.
    pub fn crash_modal_is_open(&self) -> bool {
        #[cfg(target_arch = "wasm32")]
        {
            self.inner.crash_modal_is_open()
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            false
        }
    }

    /// Get crash modal message.
    pub fn crash_modal_message(&self) -> Option<String> {
        #[cfg(target_arch = "wasm32")]
        {
            self.inner.crash_modal_message().map(|s| s.to_string())
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            None
        }
    }

    /// Check if the app should exit.
    pub fn should_exit(&self) -> bool {
        #[cfg(target_arch = "wasm32")]
        {
            self.inner.should_exit()
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            false
        }
    }

    /// Check if multiline mode is active.
    pub fn multiline_mode(&self) -> bool {
        #[cfg(target_arch = "wasm32")]
        {
            self.inner.multiline_mode()
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            false
        }
    }
}
