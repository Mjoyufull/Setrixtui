//! App: terminal init, main loop, tick and key handling.

use crate::game::GameState;
use crate::input::{Action, key_to_action};
use crate::theme::Theme;
use crate::{Args, GameConfig};
use anyhow::Result;
use ratatui::{
    DefaultTerminal,
    crossterm::event::{self, Event, KeyCode, KeyEventKind},
};
use std::time::{Duration, Instant};
use tachyonfx::Effect;

/// DAS (Delayed Auto-Shift): delay before movement starts repeating when you hold a key.
const REPEAT_DELAY_MS: u64 = 80;
/// ARR (Auto-Repeat Rate): time between repeated moves while holding.
const REPEAT_INTERVAL_MS: u64 = 38;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Menu,
    Playing,
    GameOver,
    QuitMenu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuitOption {
    Resume,
    MainMenu,
    Exit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameOverReason {
    StackOverflow,
    TimeUp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuTab {
    Difficulty,
    Mode,
    Autoplay,
    AutoRestart,
    Start,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuState {
    pub current_tab: MenuTab,
    pub selected_difficulty: crate::Difficulty,
    pub selected_mode: crate::GameMode,
    pub animation_start: Instant,
    pub ratman_typed: String,
    pub ratman_unlocked: bool,
    pub autoplay_enabled: bool,
    pub auto_restart_enabled: bool,
}

impl Default for MenuState {
    fn default() -> Self {
        Self {
            current_tab: MenuTab::Difficulty,
            selected_difficulty: crate::Difficulty::Easy,
            selected_mode: crate::GameMode::Endless,
            animation_start: Instant::now(),
            ratman_typed: String::new(),
            ratman_unlocked: false,
            autoplay_enabled: false,
            auto_restart_enabled: false,
        }
    }
}

pub struct App {
    args: Args,
    config: GameConfig,
    theme: Theme,
    /// Playfield size clamped to terminal so board + border fit on screen.
    effective_playfield_width: u16,
    effective_playfield_height: u16,
    state: GameState,
    screen: Screen,
    paused: bool,
    game_start: Instant,
    game_over_reason: Option<GameOverReason>,
    last_tick: Instant,
    /// Base tick rate (Hz) for level-based speed when not relaxed.
    base_tick_rate: f64,
    repeat_state: Option<(Action, Instant)>,
    last_repeat_fire: Option<Instant>,
    last_input_time: Instant,
    line_clear_started: Option<Instant>,
    /// `TachyonFX` fade effect for line-clear (created when animation starts).
    line_clear_effect: Option<Effect>,
    /// Last time we processed the line-clear effect (for delta).
    line_clear_effect_process_time: Option<Instant>,
    menu_state: MenuState,
    quit_selected: QuitOption,
    high_score_endless: u32,
    high_score_timed: u32,
    high_score_clear: u32,
    /// High scores at the start of the current game (for "New record!").
    high_score_at_game_start: (u32, u32, u32),
    /// True if this game set a new record for the current mode (used on game over screen).
    new_high_score_this_game: bool,
    /// When in Clear40: time (secs) when player first reached 40 lines; None until then.
    time_to_40_secs: Option<u64>,
    /// Playfield size from current terminal when on menu (zoom out = bigger). Used when starting from menu; during play size is fixed.
    menu_playfield_width: u16,
    menu_playfield_height: u16,
    last_frame_time: Instant,
    autoplay: bool,
    autoplay_moves: std::collections::VecDeque<crate::input::Action>,
    last_autoplay_action: Instant,
    /// True while waiting for frozen grains to drain after a hard-drop.
    autoplay_settling: bool,
    auto_restart: bool,
}

const fn default_tick_rate_for_difficulty(d: crate::Difficulty) -> f64 {
    match d {
        crate::Difficulty::Easy => 30.0,
        crate::Difficulty::Medium => 50.0,
        crate::Difficulty::Hard => 90.0,
    }
}

impl App {
    #[allow(clippy::needless_pass_by_value, clippy::unnecessary_wraps)]
    pub fn new(args: Args, config: GameConfig, theme: Theme) -> Result<Self> {
        let (high_score_endless, high_score_timed, high_score_clear) =
            crate::highscores::load_high_scores();
        let width = crate::effective_playfield_width(args.difficulty, args.width);
        let height = args.height;

        let autoplay = if args.no_menu { args.autoplay } else { false };
        let auto_restart = if args.no_menu { args.auto_restart } else { false };

        #[allow(clippy::needless_borrow)]
        let state = GameState::new(theme.clone(), width, height, &config);
        #[allow(clippy::float_cmp)]
        let tick_rate = if args.tick_rate == 18.0 {
            default_tick_rate_for_difficulty(args.difficulty)
        } else {
            args.tick_rate
        };
        let screen = if args.no_menu {
            Screen::Playing
        } else {
            Screen::Menu
        };
        let now = Instant::now();

        let mut menu_state = MenuState::default();
        menu_state.autoplay_enabled = args.autoplay;
        menu_state.auto_restart_enabled = args.auto_restart;
        menu_state.selected_difficulty = args.difficulty;
        menu_state.selected_mode = args.mode;

        Ok(Self {
            args,
            config,
            theme,
            effective_playfield_width: width,
            effective_playfield_height: height,
            state,
            screen,
            paused: false,
            game_start: now,
            game_over_reason: None,
            last_tick: now,
            base_tick_rate: tick_rate,
            repeat_state: None,
            last_repeat_fire: None,
            last_input_time: now,
            line_clear_started: None,
            line_clear_effect: None,
            line_clear_effect_process_time: None,
            menu_state,
            quit_selected: QuitOption::Resume,
            high_score_endless,
            high_score_timed,
            high_score_clear,
            high_score_at_game_start: (high_score_endless, high_score_timed, high_score_clear),
            new_high_score_this_game: false,
            time_to_40_secs: None,
            menu_playfield_width: width,
            menu_playfield_height: height,
            last_frame_time: now,
            autoplay,
            autoplay_moves: std::collections::VecDeque::new(),
            last_autoplay_action: now,
            autoplay_settling: false,
            auto_restart,
        })
    }

    /// Reset game to initial state. If `to_playing` is true, transitions to Playing screen.
    pub fn reset_game(&mut self, to_playing: bool) {
        let prev_screen = self.screen;
        let width = self.effective_playfield_width;
        let height = self.effective_playfield_height;
        let now = Instant::now();
        let old_menu_state = self.menu_state.clone();


        // Recalculate base tick rate according to current difficulty
        self.base_tick_rate = default_tick_rate_for_difficulty(self.args.difficulty);

        self.state = GameState::new(self.theme.clone(), width, height, &self.config);
        self.paused = false;
        self.game_start = now;
        self.game_over_reason = None;
        self.last_tick = now;
        self.last_input_time = now;
        self.repeat_state = None;
        self.last_repeat_fire = None;
        self.line_clear_started = None;
        self.line_clear_effect = None;
        self.line_clear_effect_process_time = None;
        self.menu_state = old_menu_state;
        self.high_score_at_game_start = (
            self.high_score_endless,
            self.high_score_timed,
            self.high_score_clear,
        );
        self.new_high_score_this_game = false;
        self.time_to_40_secs = None;
        self.autoplay_moves.clear();
        self.autoplay_settling = false;

        if self.menu_state.ratman_unlocked {
            self.args.high_color = true;
            self.state = GameState::new(
                self.theme.clone(),
                self.effective_playfield_width,
                self.effective_playfield_height,
                &self.config,
            );
            self.state.high_color = true;
            self.base_tick_rate *= 1.5;
        }

        if to_playing {
            self.screen = Screen::Playing;
        } else if prev_screen == Screen::Menu && self.autoplay {
             self.screen = Screen::Menu;
        } else {
             self.screen = Screen::Playing;
        }
    }

    fn apply_action(&mut self, action: Action, now: Instant) {
        match action {
            Action::Quit | Action::Pause | Action::None => {}
            Action::MoveLeft => self.state.move_left(now),
            Action::MoveRight => self.state.move_right(now),
            Action::RotateCw => self.state.rotate_cw(now),
            Action::RotateCcw => self.state.rotate_ccw(now),
            Action::SoftDrop => self.state.soft_drop(now),
            Action::HardDrop => {
                self.state.hard_drop(now);
                self.repeat_state = None;
            }
        }
    }

    fn tick_repeat(&mut self) {
        let now = Instant::now();
        let Some((action, first)) = self.repeat_state else {
            return;
        };
        if action == Action::Quit
            || action == Action::HardDrop
            || action == Action::Pause
            || action == Action::None
        {
            return;
        }

        // Removed safety fallback that assumed sticky keys after 100ms;
        // now relying on KeyEventKind::Release and standard DAS/ARR logic.

        if first.elapsed() < Duration::from_millis(REPEAT_DELAY_MS) {
            return;
        }
        let next =
            self.last_repeat_fire.unwrap_or(first) + Duration::from_millis(REPEAT_INTERVAL_MS);
        if now >= next {
            self.apply_action(action, now);
            if matches!(
                action,
                Action::MoveLeft | Action::MoveRight | Action::RotateCw | Action::RotateCcw
            ) {
                self.state.on_move_or_rotate(now);
            }
            self.last_repeat_fire = Some(now);
        }
    }

    pub fn run(&mut self) -> Result<()> {
        use ratatui::crossterm::{
            event::{
                KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
            },
            execute,
            terminal::{
                EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode, size,
            },
        };

        enable_raw_mode()?;
        let mut stdout = std::io::stdout();
        execute!(stdout, EnterAlternateScreen)?;

        // Attempt to enable enhanced keyboard for Release events
        let _ = execute!(
            stdout,
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::REPORT_EVENT_TYPES)
        );

        let mut terminal =
            ratatui::DefaultTerminal::new(ratatui::backend::CrosstermBackend::new(stdout))?;

        // Size playfield to fit terminal (no squeeze); respect --width/--height when they fit
        let (term_cols, term_rows) = size()?;
        let (fit_w, fit_h) = crate::ui::playfield_size_for_terminal_clamped(term_cols, term_rows);
        let requested_w = crate::effective_playfield_width(self.args.difficulty, self.args.width);
        let requested_h = self.args.height;
        self.effective_playfield_width = requested_w.min(fit_w).max(1);
        self.effective_playfield_height = requested_h.min(fit_h).max(1);
        self.menu_playfield_width = self.effective_playfield_width;
        self.menu_playfield_height = self.effective_playfield_height;
        let need_resize = self.state.playfield.width != self.effective_playfield_width as usize
            || self.state.playfield.height != self.effective_playfield_height as usize;
        if need_resize {
            self.state = GameState::new(
                self.theme.clone(),
                self.effective_playfield_width,
                self.effective_playfield_height,
                &self.config,
            );
        }

        let result = self.run_loop(&mut terminal);

        // Restore
        let _ = execute!(std::io::stdout(), PopKeyboardEnhancementFlags);
        execute!(std::io::stdout(), LeaveAlternateScreen)?;
        disable_raw_mode()?;

        result
    }

    #[allow(clippy::too_many_lines)]
    fn run_loop(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        loop {
            let now = Instant::now();
            let dt_secs = now.duration_since(self.last_frame_time).as_secs_f32();
            self.last_frame_time = now;
            self.state.tick_piece_visual(dt_secs);
            if self.screen == Screen::Menu {
                let (c, r) = ratatui::crossterm::terminal::size().unwrap_or((80, 24));
                let (w, h) = crate::ui::playfield_size_for_terminal_clamped(c, r);
                self.menu_playfield_width = w;
                self.menu_playfield_height = h;
            }
            let menu_size = (self.screen == Screen::Menu)
                .then_some((self.menu_playfield_width, self.menu_playfield_height));
            terminal.draw(|f| {
                crate::ui::draw(
                    f,
                    self.screen,
                    &self.state,
                    self.paused,
                    self.game_over_reason,
                    self.args.mode,
                    self.args.clear_lines,
                    self.args.time_limit,
                    self.game_start,
                    f.area(),
                    &mut self.line_clear_effect,
                    &mut self.line_clear_effect_process_time,
                    &mut self.menu_state,
                    now,
                    self.args.no_animation,
                    if self.screen == Screen::QuitMenu {
                        Some(self.quit_selected)
                    } else {
                        None
                    },
                    menu_size,
                    (
                        self.high_score_endless,
                        self.high_score_timed,
                        self.high_score_clear,
                    ),
                    self.new_high_score_this_game,
                    self.time_to_40_secs,
                    self.autoplay,
                );
            })?;

            if self.state.line_clear_in_progress
                && !self.args.no_animation
                && self.line_clear_effect.as_ref().is_some_and(Effect::done)
            {
                self.state.finish_line_clear();
                self.line_clear_effect = None;
                self.line_clear_effect_process_time = None;
                self.line_clear_started = None;
            }

            let mut rate = if self.args.relaxed {
                self.base_tick_rate
            } else {
                self.base_tick_rate * (1.0 + f64::from(self.state.level.saturating_sub(1)) * 0.1)
            };

            if self.menu_state.ratman_unlocked {
                rate *= 2.0;
            }

            let tick_interval = Duration::from_secs_f64(1.0 / rate);

            // Higher refresh rate for smooth movement and responsive input (4ms ≈ 250 FPS)
            let frame_duration = Duration::from_millis(4);
            let loop_elapsed = now.elapsed();
            let timeout = frame_duration.saturating_sub(loop_elapsed);


            // Tick popups
            self.state.tick_popups(16);

            // Timed mode check
            if self.screen == Screen::Playing && self.args.mode == crate::GameMode::Timed {
                let elapsed = now.duration_since(self.game_start).as_secs();
                if elapsed >= u64::from(self.args.time_limit) {
                    self.screen = Screen::GameOver;
                    self.game_over_reason = Some(GameOverReason::TimeUp);
                }
            }

            // High score update (during play for Endless/Timed; Clear is updated on win below)
            match self.args.mode {
                crate::GameMode::Endless => {
                    if self.state.score > self.high_score_endless {
                        self.high_score_endless = self.state.score;
                        self.new_high_score_this_game = true;
                        if !self.autoplay {
                            let _ = crate::highscores::save_high_scores(
                                self.high_score_endless,
                                self.high_score_timed,
                                self.high_score_clear,
                            );
                        }
                    }
                }
                crate::GameMode::Timed => {
                    if self.state.score > self.high_score_timed {
                        self.high_score_timed = self.state.score;
                        self.new_high_score_this_game = true;
                        if !self.autoplay {
                            let _ = crate::highscores::save_high_scores(
                                self.high_score_endless,
                                self.high_score_timed,
                                self.high_score_clear,
                            );
                        }
                    }
                }
                crate::GameMode::Clear => {}
            }

            if event::poll(timeout)? {
                while event::poll(Duration::ZERO)? {
                    if let Event::Key(key) = event::read()? {
                        let action = key_to_action(key);
                        self.last_input_time = Instant::now();

                        // Ignore OS repeats and only process first Press.
                        // Filter out redundant OS presses if we're already repeating that action ourselves.
                        if key.kind != KeyEventKind::Press {
                            if key.kind == KeyEventKind::Release
                                && self.repeat_state.map(|(a, _)| a) == Some(action)
                            {
                                self.repeat_state = None;
                                self.last_repeat_fire = None;
                            }
                            continue;
                        }

                        // If we are already repeating this action, ignore subsequent OS Press events
                        if self.repeat_state.map(|(a, _)| a) == Some(action) {
                            continue;
                        }

                        match self.screen {
                            Screen::Menu => {
                                match action {
                                    Action::Quit => return Ok(()),
                                    Action::MoveLeft => match self.menu_state.current_tab {
                                        MenuTab::Difficulty => {
                                            self.menu_state.selected_difficulty = match self
                                                .menu_state
                                                .selected_difficulty
                                            {
                                                crate::Difficulty::Easy => crate::Difficulty::Hard,
                                                crate::Difficulty::Medium => {
                                                    crate::Difficulty::Easy
                                                }
                                                crate::Difficulty::Hard => {
                                                    crate::Difficulty::Medium
                                                }
                                            };
                                        }
                                        MenuTab::Mode => {
                                            self.menu_state.selected_mode = match self
                                                .menu_state
                                                .selected_mode
                                            {
                                                crate::GameMode::Endless => crate::GameMode::Clear,
                                                crate::GameMode::Timed => crate::GameMode::Endless,
                                                crate::GameMode::Clear => crate::GameMode::Timed,
                                            };
                                        }
                                        MenuTab::Autoplay => {
                                            // Move to AutoRestart (wrap or side?)
                                            // Side-by-side means Left from Autoplay might wrap to AutoRestart or do nothing?
                                            // Let's make it circular for the row.
                                            self.menu_state.current_tab = MenuTab::AutoRestart;
                                        }
                                        MenuTab::AutoRestart => {
                                            self.menu_state.current_tab = MenuTab::Autoplay;
                                        }
                                        MenuTab::Start => {}
                                    },
                                    Action::MoveRight => match self.menu_state.current_tab {
                                        MenuTab::Difficulty => {
                                            self.menu_state.selected_difficulty = match self
                                                .menu_state
                                                .selected_difficulty
                                            {
                                                crate::Difficulty::Easy => {
                                                    crate::Difficulty::Medium
                                                }
                                                crate::Difficulty::Medium => {
                                                    crate::Difficulty::Hard
                                                }
                                                crate::Difficulty::Hard => crate::Difficulty::Easy,
                                            };
                                        }
                                        MenuTab::Mode => {
                                            self.menu_state.selected_mode = match self
                                                .menu_state
                                                .selected_mode
                                            {
                                                crate::GameMode::Endless => crate::GameMode::Timed,
                                                crate::GameMode::Timed => crate::GameMode::Clear,
                                                crate::GameMode::Clear => crate::GameMode::Endless,
                                            };
                                        }
                                        MenuTab::Autoplay => {
                                            self.menu_state.current_tab = MenuTab::AutoRestart;
                                        }
                                        MenuTab::AutoRestart => {
                                            self.menu_state.current_tab = MenuTab::Autoplay;
                                        }
                                        MenuTab::Start => {}
                                    },
                                    Action::SoftDrop => {
                                        self.menu_state.current_tab =
                                            match self.menu_state.current_tab {
                                                MenuTab::Difficulty => MenuTab::Mode,
                                                MenuTab::Mode => MenuTab::Autoplay,
                                                MenuTab::Autoplay | MenuTab::AutoRestart => MenuTab::Start,
                                                MenuTab::Start => MenuTab::Difficulty,
                                            };
                                    }
                                    Action::RotateCw | Action::RotateCcw => {
                                        self.menu_state.current_tab =
                                            match self.menu_state.current_tab {
                                                MenuTab::Difficulty => MenuTab::Start,
                                                MenuTab::Mode => MenuTab::Difficulty,
                                                MenuTab::Autoplay | MenuTab::AutoRestart => MenuTab::Mode,
                                                MenuTab::Start => MenuTab::Autoplay,
                                            };
                                    }
                                    Action::HardDrop => {
                                        if self.menu_state.current_tab == MenuTab::Start {
                                            self.args.difficulty =
                                                self.menu_state.selected_difficulty;
                                            self.args.mode = self.menu_state.selected_mode;
                                            self.config.difficulty = self.args.difficulty;
                                            self.effective_playfield_width =
                                                self.menu_playfield_width;
                                            self.effective_playfield_height =
                                                self.menu_playfield_height;
                                            // Apply autoplay setting from menu
                                            self.autoplay = self.menu_state.autoplay_enabled;
                                            self.auto_restart = self.menu_state.auto_restart_enabled;
                                            self.reset_game(true);
                                        } else if self.menu_state.current_tab == MenuTab::Autoplay {
                                            // Toggle autoplay with Enter/HardDrop
                                             self.menu_state.autoplay_enabled = !self.menu_state.autoplay_enabled;
                                        } else if self.menu_state.current_tab == MenuTab::AutoRestart {
                                             self.menu_state.auto_restart_enabled = !self.menu_state.auto_restart_enabled;
                                        } else {
                                            self.menu_state.current_tab = MenuTab::Start;
                                        }
                                    }
                                    _ => {
                                        if let KeyCode::Char(c) = key.code {
                                            self.menu_state.ratman_typed.push(c);
                                            if "Ratman".starts_with(&self.menu_state.ratman_typed) {
                                                if self.menu_state.ratman_typed == "Ratman" {
                                                    self.menu_state.ratman_unlocked = true;
                                                }
                                            } else {
                                                self.menu_state.ratman_typed = c.to_string();
                                                if c == 'R' {
                                                    // Start over maybe?
                                                } else {
                                                    self.menu_state.ratman_typed.clear();
                                                }
                                            }
                                        }

                                        if key.code == KeyCode::Enter {
                                            if self.menu_state.current_tab == MenuTab::Start {
                                                self.args.difficulty =
                                                    self.menu_state.selected_difficulty;
                                                self.args.mode = self.menu_state.selected_mode;
                                                self.config.difficulty = self.args.difficulty;
                                                self.effective_playfield_width =
                                                    self.menu_playfield_width;
                                                self.effective_playfield_height =
                                                    self.menu_playfield_height;
                                                self.reset_game(true);
                                            } else {
                                                self.menu_state.current_tab = MenuTab::Start;
                                            }
                                        }
                                    }
                                }
                            }
                            Screen::Playing => {
                                if self.paused {
                                    if action == Action::Pause {
                                        self.paused = false;
                                    } else if action == Action::Quit {
                                        self.screen = Screen::QuitMenu;
                                        self.quit_selected = QuitOption::Resume;
                                    }
                                } else {
                                    match action {
                                        Action::Pause => self.paused = true,
                                        Action::Quit => {
                                            self.screen = Screen::QuitMenu;
                                            self.quit_selected = QuitOption::Resume;
                                        }
                                        Action::MoveLeft | Action::MoveRight | Action::RotateCw 
                                        | Action::RotateCcw | Action::SoftDrop | Action::HardDrop => {
                                             self.apply_action(action, now);
                                             if matches!(action, Action::MoveLeft | Action::MoveRight 
                                                 | Action::RotateCw | Action::RotateCcw) {
                                                 self.state.on_move_or_rotate(now);
                                             }
                                        }
                                        _ => {}
                                    }
                                    
                                    let repeatable = matches!(
                                        action,
                                        Action::MoveLeft | Action::MoveRight | Action::SoftDrop
                                    );
                                    if repeatable {
                                        self.repeat_state = Some((action, now));
                                        self.last_repeat_fire = None;
                                    }
                                }

                                // If the action caused a lock, clear repeat state to prevent "input memory"
                                if self.state.line_clear_in_progress
                                    || self.state.piece.is_none()
                                {
                                    self.repeat_state = None;
                                }
                            }
                            Screen::QuitMenu => {
                                match action {
                                    Action::SoftDrop | Action::MoveRight => {
                                        // Cycle Down
                                        self.quit_selected = match self.quit_selected {
                                            QuitOption::Resume => QuitOption::MainMenu,
                                            QuitOption::MainMenu => QuitOption::Exit,
                                            QuitOption::Exit => QuitOption::Resume,
                                        };
                                    }
                                    Action::RotateCw | Action::RotateCcw | Action::MoveLeft => {
                                        // Cycle Up
                                        self.quit_selected = match self.quit_selected {
                                            QuitOption::Resume => QuitOption::Exit,
                                            QuitOption::MainMenu => QuitOption::Resume,
                                            QuitOption::Exit => QuitOption::MainMenu,
                                        };
                                    }
                                    Action::HardDrop => match self.quit_selected {
                                        QuitOption::Resume => self.screen = Screen::Playing,
                                        QuitOption::MainMenu => {
                                            self.autoplay = false;
                                            self.auto_restart = false;
                                            self.screen = Screen::Menu;
                                        }
                                        QuitOption::Exit => return Ok(()),
                                    },
                                    Action::Pause | Action::Quit => {
                                        self.screen = Screen::Playing;
                                    }
                                    Action::None => {
                                        // If user hits Enter/Space directly via Action::HardDrop it confirm.
                                        // The SoftDrop (Down) and RotateCw (Up) are now mapped to cycling.
                                    }
                                }
                            }
                            Screen::GameOver => {
                                if action == Action::Quit {
                                    return Ok(());
                                }
                                if key.code == KeyCode::Char('r') || key.code == KeyCode::Char('R')
                                {
                                    self.reset_game(true);
                                }
                            }
                        }
                    }
                }
            }
            
            // Should we tick game logic?
            // Yes if playing, OR if in Menu and autoplay is enabled (background preview)
            let should_tick = (self.screen == Screen::Playing && !self.paused) 
                || (self.screen == Screen::Menu && self.autoplay);

            if should_tick {
                self.tick_game_logic(tick_interval);
            }
        }
    }

    fn tick_game_logic(&mut self, tick_interval: Duration) {
        // --- AUTOPLAY LOGIC ---
        // Bot actions are throttled to the game's tick rate and must wait
        // for frozen grains to fully settle between placements.
        if self.autoplay {
            let now_ap = Instant::now();
            let action_delay = tick_interval.mul_f64(2.0);

            // If settling after a hard-drop, wait for physics to finish.
            if self.autoplay_settling {
                if self.state.frozen_grains.is_empty()
                    && self.state.crumble_delay_ticks == 0
                    && !self.state.line_clear_in_progress
                {
                    // Physics done — allow next piece.
                    self.autoplay_settling = false;
                    self.last_autoplay_action = now_ap;
                }
            } else if !self.state.game_over
                && !self.state.line_clear_in_progress
                && self.state.piece.is_some()
                && now_ap.duration_since(self.last_autoplay_action) >= action_delay
            {
                // Compute move if queue is empty.
                if self.autoplay_moves.is_empty() {
                    self.autoplay_moves = crate::autoplay::Bot::find_best_move(&self.state);
                }

                if let Some(auto_action) = self.autoplay_moves.pop_front() {
                    self.apply_action(auto_action, now_ap);
                    if matches!(
                        auto_action,
                        Action::MoveLeft
                            | Action::MoveRight
                            | Action::RotateCw
                            | Action::RotateCcw
                    ) {
                        self.state.on_move_or_rotate(now_ap);
                    }
                    self.last_autoplay_action = now_ap;

                    // After hard-drop, enter settling mode.
                    if auto_action == Action::HardDrop {
                        self.autoplay_settling = true;
                    }
                }
            }
        }

        self.tick_repeat();
        if self.last_tick.elapsed() >= tick_interval {
            self.last_tick = Instant::now();
            self.state.tick_gravity(Instant::now());

            let steps = if self.menu_state.ratman_unlocked {
                2
            } else {
                1
            };
            for _ in 0..steps {
                self.state.tick_sand();
            }
        }

        // Check for locking EVERY frame for maximum "snappiness"
        self.state.check_lock(Instant::now());

        // --- DYNAMIC CLEAR CHECK ---
        if self.args.mode == crate::GameMode::Clear
            && self.time_to_40_secs.is_none()
            && self.state.lines_cleared >= self.args.clear_lines
        {
            self.time_to_40_secs = Some(self.game_start.elapsed().as_secs());
        }
        
        // Game Over Logic
        if self.state.game_over {
            // AUTO RESTART LOGIC
            if self.autoplay && self.auto_restart {
                self.reset_game(false);
                return;
            }

            self.game_over_reason = Some(GameOverReason::StackOverflow);

            match self.args.mode {
                crate::GameMode::Endless => {
                    if self.state.score > self.high_score_endless {
                        self.high_score_endless = self.state.score;
                        self.new_high_score_this_game = true;
                        if !self.autoplay {
                            let _ = crate::highscores::save_high_scores(
                                self.high_score_endless,
                                self.high_score_timed,
                                self.high_score_clear,
                            );
                        }
                    }
                }
                crate::GameMode::Timed => {
                    if self.state.score > self.high_score_timed {
                        self.high_score_timed = self.state.score;
                        self.new_high_score_this_game = true;
                        if !self.autoplay {
                             let _ = crate::highscores::save_high_scores(
                                self.high_score_endless,
                                self.high_score_timed,
                                self.high_score_clear,
                            );
                        }
                    }
                }
                crate::GameMode::Clear => {
                    if self.state.lines_cleared > self.high_score_clear {
                        self.high_score_clear = self.state.lines_cleared;
                        self.new_high_score_this_game = true;
                        if !self.autoplay {
                            let _ = crate::highscores::save_high_scores(
                                self.high_score_endless,
                                self.high_score_timed,
                                self.high_score_clear,
                            );
                        }
                    }
                }
            }
            // If in Menu and we fail, we probably want to restart anyway?
            // If we are showing "background play", game over just resets?
            // If not auto-restart, we show game over.
            // If in menu, showing game over screen is weird.
            // If in menu, we should probably just reset silently.
            if self.screen == Screen::Menu {
                 self.reset_game(false);
            } else {
                 self.screen = Screen::GameOver;
            }
        } else if self.args.mode == crate::GameMode::Timed
            && self.game_start.elapsed() >= Duration::from_secs(u64::from(self.args.time_limit))
        {
            self.game_over_reason = Some(GameOverReason::TimeUp);
            if self.state.score > self.high_score_timed {
                self.high_score_timed = self.state.score;
                self.new_high_score_this_game = true;
                if !self.autoplay {
                    let _ = crate::highscores::save_high_scores(
                        self.high_score_endless,
                        self.high_score_timed,
                        self.high_score_clear,
                    );
                }
            }
            if self.screen == Screen::Menu {
                 self.reset_game(false);
            } else {
                 self.screen = Screen::GameOver;
            }
        }
        
        // Handle clear animation finish
        if self.state.line_clear_in_progress
             && !self.args.no_animation
             && self.line_clear_effect.as_ref().is_some_and(Effect::done)
        {
             self.state.finish_line_clear();
             self.line_clear_effect = None;
             self.line_clear_effect_process_time = None;
             self.line_clear_started = None;
        }
        // Handle instant clear (no animation)
        if self.state.line_clear_in_progress
            && !self.state.line_clear_cells.is_empty()
            && self.args.no_animation
        {
            self.state.finish_line_clear();
            self.line_clear_started = None;
            self.line_clear_effect = None;
            self.line_clear_effect_process_time = None;
        }
    }
}
