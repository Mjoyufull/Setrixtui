//! Key bindings: normal and vim-style.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Action from a key press.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    MoveLeft,
    MoveRight,
    RotateCw,
    RotateCcw,
    SoftDrop,
    HardDrop,
    Pause,
    Quit,
    None,
}

/// Map key event to game action. Supports both normal (arrows, space) and vim (hjkl, etc.).
pub fn key_to_action(key: KeyEvent) -> Action {
    let KeyEvent {
        code, modifiers, ..
    } = key;
    let no_mod = modifiers.is_empty() || modifiers == KeyModifiers::SHIFT;
    if !no_mod && modifiers != KeyModifiers::CONTROL {
        return Action::None;
    }
    match code {
        KeyCode::Char('q') | KeyCode::Esc if no_mod => Action::Quit,
        KeyCode::Char('p' | ' ') if modifiers == KeyModifiers::CONTROL => Action::Pause,
        KeyCode::Char('p') if no_mod => Action::Pause,
        KeyCode::Left | KeyCode::Char('h') if no_mod => Action::MoveLeft,
        KeyCode::Right | KeyCode::Char('l') if no_mod => Action::MoveRight,
        KeyCode::Up | KeyCode::Char('k') if no_mod => Action::RotateCw,
        KeyCode::Char('i') if no_mod => Action::RotateCw,
        KeyCode::Char('u') if no_mod => Action::RotateCcw,
        KeyCode::Down | KeyCode::Char('j') if no_mod => Action::SoftDrop,
        KeyCode::Enter | KeyCode::Char(' ') if no_mod => Action::HardDrop,
        _ => Action::None,
    }
}
