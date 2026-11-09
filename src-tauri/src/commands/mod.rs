pub mod discord;
pub mod images;
pub mod library;
pub mod session;
pub mod settings;

use std::sync::Arc;

use tauri::State;

use crate::state::AppState;

/// Every command takes the shared state the same way.
pub type Shared<'a> = State<'a, Arc<AppState>>;
