//! Setrixtui — Setris/Sandtrix-style falling-sand puzzle game in the terminal.

mod app;
mod game;
mod input;
mod theme;
mod ui;

use anyhow::Result;
use app::App;
use clap::{Parser, ValueEnum};

/// Options derived from CLI that affect game behaviour (spawn delay, lock delay, sand settle, etc.).
#[derive(Debug, Clone)]
pub struct GameConfig {
    pub spawn_delay_ms: u64,
    pub initial_level: u32,
    pub lock_delay_ms: u64,
    pub sand_settle: bool,
    pub relaxed: bool,
    pub high_color: bool,
    pub difficulty: Difficulty,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let theme = theme::Theme::load(args.theme.as_deref(), args.palette).unwrap_or_default();
    let config = GameConfig {
        spawn_delay_ms: args.spawn_delay_ms.unwrap_or(0),
        initial_level: args.initial_level,
        lock_delay_ms: args.lock_delay_ms.unwrap_or(120),
        sand_settle: args.sand_settle,
        relaxed: args.relaxed,
        high_color: args.high_color,
        difficulty: args.difficulty,
    };
    let mut app = App::new(args, config, theme)?;
    app.run()?;
    Ok(())
}

/// Setris/Sandtrix-style puzzle game in the terminal.
#[derive(Debug, Parser)]
#[command(
    name = "setrixtui",
    version,
    about = "Setris/Sandtrix-style falling-sand puzzle in the terminal. Blocks turn into sand; clear full horizontal lines to score.",
    long_about = "Setrixtui is a terminal puzzle game inspired by Setris and Sandtrix.\n\n\
        Place falling coloured blocks. When they lock, they turn into sand. Clear horizontal \
        lines (one colour edge-to-edge) to score; remaining sand falls with gravity.\n\n\
        CONTROLS (normal):\n  Left/Right  Move    Up        Rotate CW   Down       Soft drop\n  Enter/Space Hard drop   P          Pause      Q / Esc    Quit\n\n\
        CONTROLS (vim):\n  h/l         Move    k or i     Rotate CW   u          Rotate CCW\n  j           Soft drop  Space      Hard drop  p          Pause   q  Quit\n\n\
        Hold a movement key to keep the piece moving. Use --theme to load a btop-style theme (e.g. onedark.theme)."
)]
pub struct Args {
    /// Game mode: endless (play until game over), timed (score in 3 min), or clear (clear N lines to win).
    #[arg(short, long, default_value = "endless")]
    pub mode: GameMode,

    /// Difficulty: easy (normal speed), medium (faster), hard (fast + narrower). Affects gravity and playfield.
    #[arg(short, long, default_value = "easy")]
    pub difficulty: Difficulty,

    /// Path to theme file (btop-style theme[key]=\"value\"). Uses One Dark if not set.
    #[arg(short, long, value_name = "FILE")]
    pub theme: Option<std::path::PathBuf>,

    /// Playfield width in columns (grid cells). Defaulting to 10 for 1080p compatibility.
    #[arg(long, default_value = "10", value_name = "COLS")]
    pub width: u16,

    /// Playfield height in rows (grid cells).
    #[arg(long, default_value = "24", value_name = "ROWS")]
    pub height: u16,

    /// In mode 'clear': number of lines to clear to win.
    #[arg(long, default_value = "40", value_name = "N")]
    pub clear_lines: u32,

    /// In mode 'timed': time limit in seconds.
    #[arg(long, default_value = "180", value_name = "SECS")]
    pub time_limit: u32,

    /// Disable line-clear animation (instant clear + gravity).
    #[arg(long)]
    pub no_animation: bool,

    /// Game logic ticks per second (gravity, lock delay).
    #[arg(long, default_value = "18.0", value_name = "RATE")]
    pub tick_rate: f64,

    /// Target render frames per second.
    #[arg(long, default_value = "25.0", value_name = "RATE")]
    pub frame_rate: f64,

    /// Skip main menu and start game immediately.
    #[arg(long)]
    pub no_menu: bool,

    /// Spawn delay in ms: piece is not controllable and gravity does not apply until after this delay (prevents instant lock on spawn).
    #[arg(long, value_name = "MS")]
    pub spawn_delay_ms: Option<u64>,

    /// Relaxed mode: gravity speed does not increase with level (fixed speed).
    #[arg(long)]
    pub relaxed: bool,

    /// Initial level (e.g. for custom / practice). Affects starting speed when not relaxed.
    #[arg(long, default_value = "1", value_name = "N")]
    pub initial_level: u32,

    /// Lock delay in ms when piece lands (before it locks). Overrides default 200 ms.
    #[arg(long, value_name = "MS")]
    pub lock_delay_ms: Option<u64>,

    /// Sand settling: after lock, sand can fall sideways (down-left/down-right) when directly below is blocked.
    #[arg(long)]
    pub sand_settle: bool,

    /// High color mode: use 6 colors (red, blue, yellow, green, magenta, cyan) instead of 4 (red, blue, yellow, green).
    #[arg(long)]
    pub high_color: bool,

    /// Colour palette: normal (theme), high-contrast, or colorblind.
    #[arg(long, default_value = "normal")]
    pub palette: Palette,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum Palette {
    #[default]
    Normal,

    #[value(alias = "highcontrast", alias = "contrast")]
    HighContrast,

    #[value(alias = "colourblind")]
    Colorblind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum GameMode {
    #[default]
    Endless,
    Timed,
    Clear,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum Difficulty {
    #[default]
    Easy,
    Medium,
    Hard,
}

/// Playfield width (no difficulty override).
pub fn effective_playfield_width(_difficulty: Difficulty, width: u16) -> u16 {
    width
}

