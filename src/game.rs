//! Game state: playfield, piece, sand, line clear, gravity.

use crate::theme::Theme;
use ratatui::style::Color;
use std::collections::{HashMap, HashSet, VecDeque};
use std::time::Instant;
use fastrand::Rng;

/// Scale factor: each tetromino block is `GRAIN_SCALE` x `GRAIN_SCALE` grains.
pub const GRAIN_SCALE: usize = 6;

/// Spawn zone: top N physical rows.
const SPAWN_ZONE_ROWS: usize = 2 * GRAIN_SCALE;

/// After this many move/rotate resets, piece locks on next land immediately.
const LOCK_DELAY_RESET_LIMIT: u32 = 15;

/// Tetromino kinds (I, O, T, S, Z, J, L).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TetrominoKind {
    I,
    O,
    T,
    S,
    Z,
    J,
    L,
}

impl TetrominoKind {
    pub const ALL: [Self; 7] = [
        Self::I,
        Self::O,
        Self::T,
        Self::S,
        Self::Z,
        Self::J,
        Self::L,
    ];

    /// 4 cells relative to origin (0,0); each (dx, dy).
    pub const fn cells(&self) -> &[(i8, i8); 4] {
        match self {
            Self::I => &[(0, 0), (1, 0), (2, 0), (3, 0)],
            Self::O => &[(0, 0), (1, 0), (0, 1), (1, 1)],
            Self::T => &[(0, 0), (1, 0), (2, 0), (1, 1)],
            Self::S => &[(1, 0), (2, 0), (0, 1), (1, 1)],
            Self::Z => &[(0, 0), (1, 0), (1, 1), (2, 1)],
            Self::J => &[(0, 0), (0, 1), (1, 1), (2, 1)],
            Self::L => &[(2, 0), (0, 1), (1, 1), (2, 1)],
        }
    }

    /// Colour index 0..6 for `theme.sand_color()`.
    /// If `high_color` is false, maps to 0..3 (Green, Yellow, Red, Blue).
    #[allow(clippy::needless_pass_by_value)] // self is small but used by callers with &kind
    pub const fn color_index(&self, high_color: bool) -> u8 {
        if high_color {
            match self {
                Self::S => 0, // Green
                Self::O => 1, // Yellow
                Self::Z => 2, // Red
                Self::J => 3, // Blue
                Self::T => 4, // Magenta
                Self::I => 5, // Cyan
                Self::L => 2, // Orange -> Red
            }
        } else {
            match self {
                Self::S => 0, // Green
                Self::O => 1, // Yellow
                Self::Z => 2, // Red
                Self::J => 3, // Blue
                Self::T => 2, // Red
                Self::I => 3, // Blue
                Self::L => 1, // Yellow
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrozenGrain {
    pub x: usize,
    pub y: usize,
    pub color_index: u8,
    pub is_shadow: bool,
}

/// Current piece with position and rotation (0..4).
#[derive(Debug, Clone)]
pub struct Piece {
    pub kind: TetrominoKind,
    pub gx: i32,
    pub gy: i32,
    pub rotation: u8, // 0..4
    pub color_index: u8,
}

impl Piece {
    /// Returns the top-left grain coordinate for each of the 4 tetromino cells (uses logic position).
    pub fn cell_grain_origins(&self) -> [(i32, i32); 4] {
        self.cell_grain_origins_at(self.gx, self.gy)
    }

    /// Same as cell_grain_origins but with explicit offset (for smooth visual drawing).
    pub fn cell_grain_origins_at(&self, ox: i32, oy: i32) -> [(i32, i32); 4] {
        let s = GRAIN_SCALE as i32;
        if self.kind == TetrominoKind::O {
            return [(ox, oy), (ox + s, oy), (ox, oy + s), (ox + s, oy + s)];
        }
        let cells = self.kind.cells();
        let r = self.rotation % 4;
        let (cx, cy) = match self.kind {
            TetrominoKind::I => (1, 0),
            _ => (1, 1),
        };
        let mut out = [(0i32, 0i32); 4];
        for (i, (dx, dy)) in cells.iter().enumerate() {
            let (rdx, rdy) = rotate_cell(*dx, *dy, r, cx, cy);
            out[i] = (
                ox + (rdx as i32 * GRAIN_SCALE as i32),
                oy + (rdy as i32 * GRAIN_SCALE as i32),
            );
        }
        out
    }
}

fn rotate_cell(dx: i8, dy: i8, r: u8, cx: i8, cy: i8) -> (i16, i16) {
    let dx = dx - cx;
    let dy = dy - cy;
    let (dx, dy) = match r {
        0 => (dx, dy),
        1 => (-dy, dx),
        2 => (-dx, -dy),
        3 => (dy, -dx),
        _ => (dx, dy),
    };
    (i16::from(dx + cx), i16::from(dy + cy))
}

/// Single cell: either empty or sand of a given colour index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cell {
    Empty,
    Sand(u8, bool), // colour index 0..6, is_shadow
}

/// Playfield: grid of cells. y=0 is top; rows are stored [0..height].
#[derive(Debug, Clone)]
pub struct Playfield {
    pub width: usize,
    pub height: usize,
    /// rows[y][x] = cell. rows[0] is top.
    rows: VecDeque<Vec<Cell>>,
    pub tick_count: u32,
}

impl Playfield {
    pub fn new(width: u16, height: u16) -> Self {
        let (w, h) = (width as usize, height as usize);
        let (gw, gh) = (w * GRAIN_SCALE, h * GRAIN_SCALE);
        let rows = (0..gh).map(|_| vec![Cell::Empty; gw]).collect();
        Self {
            width: w,
            height: h,
            rows,
            tick_count: 0,
        }
    }

    /// Get actual grain dimensions.
    #[inline]
    pub fn grain_dims(&self) -> (usize, usize) {
        (self.width * GRAIN_SCALE, self.height * GRAIN_SCALE)
    }

    #[inline]
    pub fn get(&self, x: usize, y: usize) -> Option<Cell> {
        let (gw, gh) = self.grain_dims();
        if x >= gw || y >= gh {
            return None;
        }
        self.rows.get(y).and_then(|row| row.get(x)).copied()
    }

    #[inline]
    pub fn set(&mut self, x: usize, y: usize, cell: Cell) {
        let (gw, gh) = self.grain_dims();
        if x < gw && y < gh {
            if let Some(row) = self.rows.get_mut(y) {
                row[x] = cell;
            }
        }
    }

    /// True if piece can be placed at its current position on the piece-grid.
    /// Checked against the physical grain grid (scaled).
    pub fn can_place(&self, piece: &Piece) -> bool {
        let origins = piece.cell_grain_origins();
        let (gw, gh) = self.grain_dims();

        for (gx_origin, gy_origin) in origins {
            for dy in 0..GRAIN_SCALE as i32 {
                for dx in 0..GRAIN_SCALE as i32 {
                    let gx = gx_origin + dx;
                    let gy = gy_origin + dy;

                    // Boundary check
                    if gx < 0 || gx >= gw as i32 || gy >= gh as i32 {
                        return false;
                    }
                    if gy < 0 {
                        continue;
                    }

                    // Collision check with SAND
                    if let Some(Cell::Sand(..)) = self.get(gx as usize, gy as usize) {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// Check collision with frozen grains (during crumble animation).
    pub fn can_place_with_frozen(&self, piece: &Piece, frozen_grains: &[FrozenGrain]) -> bool {
        if !self.can_place(piece) {
            return false;
        }
        // Also check against frozen grains
        let origins = piece.cell_grain_origins();
        for (gx_origin, gy_origin) in origins {
            for dy in 0..GRAIN_SCALE as i32 {
                for dx in 0..GRAIN_SCALE as i32 {
                    let gx = gx_origin + dx;
                    let gy = gy_origin + dy;
                    if gy >= 0 {
                         for fg in frozen_grains {
                             if fg.x == gx as usize && fg.y == gy as usize {
                                 return false;
                             }
                         }
                    }
                }
            }
        }
        true
    }

    /// Edge-to-edge clear: one colour connects left (x=0) to right (x=width-1); path can be slanted (8-neighbour).
    /// Returns (number of such clears, list of (x,y) cells to clear).
    pub fn find_spanning_components(&self) -> (u32, Vec<(usize, usize)>) {
        const NEIGHBOURS_8: [(i16, i16); 8] = [
            (-1, -1),
            (-1, 0),
            (-1, 1),
            (0, -1),
            (0, 1),
            (1, -1),
            (1, 0),
            (1, 1),
        ];
        let (gw, gh) = self.grain_dims();
        let mut num_clears = 0u32;
        let mut all_to_clear = Vec::new();

        for color in 0..6u8 {
            let mut visited = HashSet::new();
            for start_y in 0..gh {
                if let Some(Cell::Sand(c, _)) = self.get(0, start_y) {
                    if c == color && !visited.contains(&(0, start_y)) {
                        let mut component = Vec::new();
                        let mut stack = vec![(0, start_y)];
                        visited.insert((0, start_y));
                        let mut touches_right = false;

                        while let Some((x, y)) = stack.pop() {
                            component.push((x, y));
                            if x == gw - 1 {
                                touches_right = true;
                            }

                            for (dx, dy) in NEIGHBOURS_8 {
                                let nx = x as i16 + dx;
                                let ny = y as i16 + dy;
                                if nx >= 0 && nx < gw as i16 && ny >= 0 && ny < gh as i16 {
                                    let (nx, ny) = (nx as usize, ny as usize);
                                    if let Some(Cell::Sand(c2, _)) = self.get(nx, ny) {
                                        if c2 == color && !visited.contains(&(nx, ny)) {
                                            visited.insert((nx, ny));
                                            stack.push((nx, ny));
                                        }
                                    }
                                }
                            }
                        }

                        if touches_right {
                            num_clears += 1;
                            all_to_clear.extend(component);
                        }
                    }
                }
            }
        }
        (num_clears, all_to_clear)
    }

    /// Unified physics step: gravity + cascading.
    /// Grains fall down, or down-left/down-right if blocked.
    pub fn tick_physics(&mut self, left_first: bool) -> bool {
        self.tick_count = self.tick_count.wrapping_add(1);
        let mut moved = false;
        let (gw, gh) = self.grain_dims();
        // Scan Entropy: Randomize x_order every frame to eliminate clumping bias.
        let mut x_order: Vec<usize> = (0..gw).collect();
        // Uses tick_count for dynamic shuffle
        let seed = self.tick_count.wrapping_mul(31).wrapping_add(gw as u32);
        // Simple swap-based shuffle
        for i in 0..gw / 4 {
            let j = (seed as usize + i) % gw;
            let k = (seed as usize * 17 + i) % gw;
            x_order.swap(j, k);
        }

        let limit_y = gh.saturating_sub(1);
        for y in (0..limit_y).rev() {
            for &x in &x_order {
                if let Some(Cell::Sand(c, is_shadow)) = self.get(x, y) {
                    // 1. Try straight down
                    if self.get(x, y + 1) == Some(Cell::Empty) {
                        self.set(x, y, Cell::Empty);
                        self.set(x, y + 1, Cell::Sand(c, is_shadow));
                        moved = true;
                    }
                    // 2. Cascading: try down-left or down-right only when blocked below
                    else {
                        let try_left = x > 0 && self.get(x - 1, y + 1) == Some(Cell::Empty);
                        let try_right = x + 1 < gw && self.get(x + 1, y + 1) == Some(Cell::Empty);

                        let go_left = if try_left && try_right {
                            left_first
                        } else {
                            try_left
                        };

                        if go_left {
                            self.set(x, y, Cell::Empty);
                            self.set(x - 1, y + 1, Cell::Sand(c, is_shadow));
                            moved = true;
                        } else if try_right {
                            self.set(x, y, Cell::Empty);
                            self.set(x + 1, y + 1, Cell::Sand(c, is_shadow));
                            moved = true;
                        }
                    }
                }
            }
        }
        moved
    }

    /// Game over if any sand in spawn zone (top SPAWN_ZONE_ROWS).
    pub fn game_over(&self) -> bool {
        self.topmost_sand_y().map_or(false, |y| y < SPAWN_ZONE_ROWS)
    }

    /// Minimum (topmost) row index that contains any sand. None if playfield has no sand.
    pub fn topmost_sand_y(&self) -> Option<usize> {
        let (gw, gh) = self.grain_dims();
        for y in 0..gh {
            for x in 0..gw {
                if matches!(self.get(x, y), Some(Cell::Sand(..))) {
                    return Some(y);
                }
            }
        }
        None
    }
}

/// Bag of 7 tetrominoes (random order, then refill).
#[derive(Debug, Clone)]
pub struct Bag {
    queue: Vec<TetrominoKind>,
    rng: Rng,
}

impl Bag {
    pub fn new(seed: u64) -> Self {
        let mut b = Self {
            queue: Vec::with_capacity(14),
            rng: Rng::with_seed(seed),
        };
        b.refill();
        b
    }

    fn refill(&mut self) {
        let mut all = TetrominoKind::ALL.to_vec();
        self.rng.shuffle(&mut all);
        self.queue.extend(all);
    }

    pub fn next(&mut self) -> TetrominoKind {
        if self.queue.len() < 2 {
            self.refill();
        }
        self.queue.remove(0)
    }
}

impl Default for Bag {
    fn default() -> Self {
        Self::new(0x1234_5678)
    }
}

#[derive(Debug, Clone)]
pub struct ScorePopup {
    pub x: usize,
    pub y: usize,
    pub amount: u32,
    pub multiplier: u32,
    pub age_ms: u32,
    pub color: Color,
}

/// Game state: playfield, current piece, next piece, score, level, etc.
#[derive(Debug, Clone)]
pub struct GameState {
    pub theme: Theme,
    pub playfield: Playfield,
    pub piece: Option<Piece>,
    pub next_pieces: Vec<(TetrominoKind, u8)>,
    pub bag: Bag,
    pub score: u32,
    pub level: u32,
    pub lines_cleared: u32,
    pub game_over: bool,
    /// Cells to clear (animation); when empty and not in_progress, we clear + gravity.
    pub line_clear_cells: Vec<(usize, usize)>,
    pub line_clear_in_progress: bool,
    /// When piece first landed (can't move down); lock after lock_delay_ms if not reset.
    lock_delay_started: Option<Instant>,
    /// Number of move/rotate resets since last land; cap at LOCK_DELAY_RESET_LIMIT.
    lock_delay_resets: u32,
    /// Spawn delay: piece not controllable / no gravity until this instant (optional).
    spawn_ready_at: Option<Instant>,
    /// Spawn delay in ms (0 = disabled).
    spawn_delay_ms: u64,
    /// High-color mode: if true, uses 6 colors; otherwise 4.
    pub high_color: bool,
    /// Settle direction bias toggle.
    settle_left_first: bool,
    pub difficulty: crate::Difficulty,
    pub popups: Vec<ScorePopup>,
    pub frozen_grains: Vec<FrozenGrain>,
    pub clears: u32,
    pub crumble_delay_ticks: u32,
    pub combo_multiplier: u32,
    pub combo_timer_ticks: u32,
    /// Visual position (grain coords) for smooth sliding; interpolates toward piece.gx/gy each frame.
    piece_visual_gx: f32,
    piece_visual_gy: f32,
    pub last_spawn_color: Option<u8>,
    pub rng: Rng,
}

impl GameState {
    pub fn new(theme: Theme, width: u16, height: u16, config: &crate::GameConfig) -> Self {
        let seed = fastrand::u64(..);
        let mut rng = Rng::with_seed(seed);
        let mut bag = Bag::new(rng.u64(..));
        let p1 = bag.next();
        let p2 = bag.next();
        let p3 = bag.next();
        let p4 = bag.next();

        let c1 = p1.color_index(config.high_color);
        let piece = Some(Self::spawn_piece(width, height, p1, c1));

        let (vx, vy) = piece
            .as_ref()
            .map(|p| (p.gx as f32, p.gy as f32))
            .unwrap_or((0.0, 0.0));
        let now = Instant::now();
        let spawn_ready_at = (config.spawn_delay_ms > 0)
            .then(|| now + std::time::Duration::from_millis(config.spawn_delay_ms));

        let mut next_pieces = Vec::new();
        let mut last_c = c1;
        for kind in [p2, p3, p4] {
            let repeat_chance = match config.difficulty {
                crate::Difficulty::Easy => 0.40,
                crate::Difficulty::Medium => 0.20,
                crate::Difficulty::Hard => 0.08,
            };
            let mut nc = kind.color_index(config.high_color);
            if rng.f32() < repeat_chance {
                nc = last_c;
            }
            next_pieces.push((kind, nc));
            last_c = nc;
        }

        Self {
            theme,
            playfield: Playfield::new(width, height),
            piece,
            next_pieces,
            bag,
            score: 0,
            level: config.initial_level,
            // ...
            lines_cleared: 0,
            game_over: false,
            line_clear_cells: Vec::new(),
            line_clear_in_progress: false,
            lock_delay_started: None,
            lock_delay_resets: 0,
            spawn_ready_at,
            spawn_delay_ms: config.spawn_delay_ms,
            high_color: config.high_color,
            settle_left_first: true,
            difficulty: config.difficulty,
            popups: Vec::new(),
            frozen_grains: Vec::new(),
            clears: 0,
            crumble_delay_ticks: 0,
            combo_multiplier: 1,
            combo_timer_ticks: 0,
            piece_visual_gx: vx,
            piece_visual_gy: vy,
            last_spawn_color: Some(c1),
            rng,
        }
    }

    /// Call every frame to smooth-slide the piece visual toward its logic position.
    pub fn tick_piece_visual(&mut self, dt_secs: f32) {
        if let Some(ref piece) = self.piece {
            let speed = 55.0; // fast catch-up: smooth slide, reaches target quickly
            let t = (dt_secs * speed).min(1.0);
            self.piece_visual_gx += (piece.gx as f32 - self.piece_visual_gx) * t;
            self.piece_visual_gy += (piece.gy as f32 - self.piece_visual_gy) * t;
        }
    }

    /// Piece origins for drawing (uses smoothed visual position so the block slides instead of snapping).
    pub fn piece_draw_origins(&self) -> Option<[(i32, i32); 4]> {
        self.piece.as_ref().map(|p| {
            p.cell_grain_origins_at(
                self.piece_visual_gx.round() as i32,
                self.piece_visual_gy.round() as i32,
            )
        })
    }

    /// True if the current piece is still in spawn delay (no gravity / no input).
    pub fn is_spawn_delay(&self, now: Instant) -> bool {
        self.spawn_ready_at.map(|t| now < t).unwrap_or(false)
    }

    pub fn spawn_piece(width: u16, _height: u16, kind: TetrominoKind, color_index: u8) -> Piece {
        let w = width as i32;
        let s = GRAIN_SCALE as i32;
        Piece {
            kind,
            gx: (w / 2 - 1).max(0) * s,
            gy: 0,
            rotation: 0,
            color_index,
        }
    }

    /// Move piece down one step if possible.
    pub fn tick_gravity(&mut self, now: Instant) {
        if self.game_over || self.line_clear_in_progress || self.is_spawn_delay(now) {
            return;
        }
        if let Some(ref mut piece) = self.piece {
            piece.gy += 1;
            let can_place = self.playfield.can_place_with_frozen(piece, &self.frozen_grains);
            if !can_place {
                piece.gy -= 1;
                // Instant crumble! The moment we hit something, it locks.
                self.lock_piece();
            } else {
                // If we moved down successfully, we are NOT landed.
                self.lock_delay_started = None;
                self.lock_delay_resets = 0;
            }
        }
        self.update_game_over_status();
    }

    /// Check if piece should lock due to time spent on ground.
    /// Call this every frame for snappy snapping.
    pub fn check_lock(&mut self, _now: Instant) {
        if self.game_over || self.line_clear_in_progress {
            return;
        }
        if let Some(ref piece) = self.piece {
            let mut test_p = piece.clone();
            test_p.gy += 1;

            if !self.playfield.can_place_with_frozen(&test_p, &self.frozen_grains) {
                // Piece is on the ground - lock instantly in Sandtrix
                self.lock_piece();
            } else {
                // Piece is in the air
                self.lock_delay_started = None;
                self.lock_delay_resets = 0;
            }
        }
    }

    /// Call when player moves or rotates; resets lock delay and increments reset count.
    pub fn on_move_or_rotate(&mut self, now: Instant) {
        if self.lock_delay_started.is_some() {
            self.lock_delay_started = Some(now);
            self.lock_delay_resets = self
                .lock_delay_resets
                .saturating_add(1)
                .min(LOCK_DELAY_RESET_LIMIT);
        }
    }

    pub fn move_left(&mut self, now: Instant) {
        if self.game_over || self.line_clear_in_progress || self.is_spawn_delay(now) {
            return;
        }
        if let Some(ref mut piece) = self.piece {
            piece.gx -= GRAIN_SCALE as i32;
            if !self.playfield.can_place_with_frozen(piece, &self.frozen_grains) {
                piece.gx += GRAIN_SCALE as i32;
            }
        }
    }

    pub fn move_right(&mut self, now: Instant) {
        if self.game_over || self.line_clear_in_progress || self.is_spawn_delay(now) {
            return;
        }
        if let Some(ref mut piece) = self.piece {
            piece.gx += GRAIN_SCALE as i32;
            if !self.playfield.can_place_with_frozen(piece, &self.frozen_grains) {
                piece.gx -= GRAIN_SCALE as i32;
            }
        }
    }

    /// Wall kick order: try 0, -1, +1, -2, +2 (SRS-style).
    pub fn rotate_cw(&mut self, now: Instant) {
        if self.game_over || self.line_clear_in_progress || self.is_spawn_delay(now) {
            return;
        }
        if let Some(ref mut piece) = self.piece {
            let old_rotation = piece.rotation;
            piece.rotation = (piece.rotation + 1) % 4;
            if !self.playfield.can_place_with_frozen(piece, &self.frozen_grains) {
                piece.rotation = old_rotation;
            }
        }
    }

    pub fn rotate_ccw(&mut self, now: Instant) {
        if self.game_over || self.line_clear_in_progress || self.is_spawn_delay(now) {
            return;
        }
        if let Some(ref mut piece) = self.piece {
            let old_rotation = piece.rotation;
            piece.rotation = (piece.rotation + 3) % 4;
            if !self.playfield.can_place_with_frozen(piece, &self.frozen_grains) {
                piece.rotation = old_rotation;
            }
        }
    }

    pub fn soft_drop(&mut self, now: Instant) {
        if self.game_over || self.line_clear_in_progress || self.is_spawn_delay(now) {
            return;
        }
        if let Some(ref mut piece) = self.piece {
            piece.gy += 1;
            if !self.playfield.can_place_with_frozen(piece, &self.frozen_grains) {
                piece.gy -= 1;
                self.lock_piece();
            } else {
                self.lock_delay_started = None;
                self.lock_delay_resets = 0;
                self.score += 1;
            }
        }
    }

    pub fn hard_drop(&mut self, now: Instant) {
        if self.game_over || self.line_clear_in_progress || self.is_spawn_delay(now) {
            return;
        }
        if let Some(piece) = &self.piece { // Avoid clone if possible, but we need mut access later
            let mut p = piece.clone();
            let (_, gh) = self.playfield.grain_dims();
            let mut pgy = p.gy;
            
            // Linear scan down
            while pgy < gh as i32 {
                p.gy = pgy + 1;
                if !self.playfield.can_place_with_frozen(&p, &self.frozen_grains) {
                    break;
                }
                pgy += 1;
            }
            
            let dist_grains = (pgy - piece.gy).max(0) as u32;
            self.score += (dist_grains / GRAIN_SCALE as u32) * 2;
            
            if let Some(ref mut p_ref) = self.piece {
                p_ref.gy = pgy;
            }
            self.lock_piece();
        }
    }

    fn lock_piece(&mut self) {
        let piece = match self.piece.take() {
            Some(p) => p,
            None => return,
        };
        let color_index = piece.color_index;

        // --- PIECE FREEZING (Freeze & Crumble) ---
        // Instead of writing to the playfield instantly, we move grains to the frozen buffer.
        // This makes the piece "freeze" in place before dissolving.
        for (gx, gy) in piece.cell_grain_origins() {
            for dy in 0..GRAIN_SCALE as i32 {
                for dx in 0..GRAIN_SCALE as i32 {
                    let px = gx + dx;
                    let py = gy + dy;

                    // Boundary check to prevent grain loss
                    if px >= 0 && py >= 0 {
                        let tx = px as usize;
                        let ty = py as usize;
                        if tx < self.playfield.width * GRAIN_SCALE
                            && ty < self.playfield.height * GRAIN_SCALE
                        {
                            // --- L-SHADOW TAGGING ---
                            // Bottom row OR Right column of each 6x6 block cell is a shadow grain.
                            // This creates persistent edge separation.
                            let is_shadow =
                                (dy == GRAIN_SCALE as i32 - 1) || (dx == GRAIN_SCALE as i32 - 1);

                            self.frozen_grains.push(FrozenGrain {
                                x: tx,
                                y: ty,
                                color_index,
                                is_shadow,
                            });
                        }
                    }
                }
            }
        }

        // --- GRAVITY-FIRST CRUMBLE ---
        // Sort grains by Y ascending so that pop() retrieves the bottom-most grains first.
        // This makes the piece dissolve from the bottom-up naturally.
        self.frozen_grains.sort_by_key(|g| g.y);

        self.crumble_delay_ticks = 5; // Freeze for 5 ticks (snappy lock) before crumbling.

        // Trigger line clear check on the playfield
        self.process_clears();

        self.update_game_over_status();
        if self.game_over {
            return;
        }
        if !self.line_clear_in_progress {
            self.spawn_next();
        }
    }

    /// Updates game_over flag when the effective top of the stack (sand or frozen) enters the spawn zone.
    /// Falling grains count as height even if they haven't landed on the main pile.
    /// We do not include the current piece: it always starts in the spawn zone and would trigger
    /// game over after the first gravity tick. Spawn-zone blockage is already handled by can_place in spawn_next.
    fn update_game_over_status(&mut self) {
        if self.game_over {
            return;
        }
        if self.playfield.game_over() {
            self.game_over = true;
            return;
        }
        if self.frozen_grains.iter().any(|g| g.y < SPAWN_ZONE_ROWS) {
            self.game_over = true;
        }
    }

    /// Called after line-clear animation: clear cells, apply gravity, spawn next.
    /// Only spawns a new piece if none is currently active (avoids replacing a mid-air piece).
    pub fn finish_line_clear(&mut self) {
        if self.line_clear_cells.is_empty() {
            self.line_clear_in_progress = false;
            if self.piece.is_none() {
                self.spawn_next();
            }
            return;
        }
        for &(x, y) in &self.line_clear_cells {
            self.playfield.set(x, y, Cell::Empty);
        }
        self.line_clear_cells.clear();
        self.line_clear_in_progress = false;
        if self.piece.is_none() {
            self.spawn_next();
        }
    }

    /// Update sand physics (one step). Should be called regularly.
    pub fn tick_sand(&mut self) {
        if self.line_clear_in_progress {
            return;
        }

        // --- PIECE CRUMBLE PROCESSOR ---
        if self.crumble_delay_ticks > 0 {
            self.crumble_delay_ticks = self.crumble_delay_ticks.saturating_sub(1);
        } else {
            // --- TURBO DRAIN (36 grains per tick) ---
            // Faster conversion (one full 6x6 block cell per logic tick).
            for _ in 0..36 {
                if let Some(fg) = self.frozen_grains.pop() {
                    self.playfield
                        .set(fg.x, fg.y, Cell::Sand(fg.color_index, fg.is_shadow));
                }
            }
        }

        // --- COMBO DECAY ---
        if self.combo_timer_ticks > 0 {
            self.combo_timer_ticks = self.combo_timer_ticks.saturating_sub(1);
            if self.combo_timer_ticks == 0 {
                self.combo_multiplier = 1;
            }
        }

        let moved = self.playfield.tick_physics(self.settle_left_first);
        self.settle_left_first = !self.settle_left_first;

        // --- DYNAMIC CLEAR CHECK (During Physics/Crumble) ---
        if (moved || (self.crumble_delay_ticks == 0 && !self.frozen_grains.is_empty()))
            && !self.line_clear_in_progress
        {
            self.process_clears();
        }
        self.update_game_over_status();
    }

    /// Check for clears and update score/popups.
    /// Clears the spanning component plus any 8-connected frozen grains of the same colour
    /// so the entire block clears at once (not just the part that had already turned to sand).
    pub fn process_clears(&mut self) {
        if self.line_clear_in_progress {
            return;
        }

        let (num, cells) = self.playfield.find_spanning_components();
        if num > 0 {
            const NEIGHBOURS_8: [(i16, i16); 8] = [
                (-1, -1),
                (-1, 0),
                (-1, 1),
                (0, -1),
                (0, 1),
                (1, -1),
                (1, 0),
                (1, 1),
            ];
            let (gw, gh) = self.playfield.grain_dims();

            // Colours that are part of the clear (spanning component)
            let clear_colors: HashSet<u8> = cells
                .iter()
                .filter_map(|&(x, y)| self.playfield.get(x, y))
                .filter_map(|c| match c {
                    Cell::Sand(ci, _) => Some(ci),
                    _ => None,
                })
                .collect();

            let mut clear_set: HashSet<(usize, usize)> = cells.iter().copied().collect();
            let frozen_map: HashMap<(usize, usize), u8> = self
                .frozen_grains
                .iter()
                .map(|fg| ((fg.x, fg.y), fg.color_index))
                .collect();

            // Expand clear set to include 8-connected frozen grains of the same colour(s)
            let mut frontier: VecDeque<(usize, usize)> = clear_set.iter().copied().collect();
            while let Some((x, y)) = frontier.pop_front() {
                for (dx, dy) in NEIGHBOURS_8 {
                    let nx = x as i16 + dx;
                    let ny = y as i16 + dy;
                    if nx >= 0 && nx < gw as i16 && ny >= 0 && ny < gh as i16 {
                        let (nx, ny) = (nx as usize, ny as usize);
                        let is_frozen_same_color = frozen_map
                            .get(&(nx, ny))
                            .map_or(false, |&c| clear_colors.contains(&c));
                        if is_frozen_same_color && clear_set.insert((nx, ny)) {
                            frontier.push_back((nx, ny));
                        }
                    }
                }
            }

            // --- COMBO SYSTEM ---
            self.combo_multiplier = (self.combo_multiplier + 1).min(10);
            self.combo_timer_ticks = 90; // 1.5s at 60Hz

            let pixel_score = clear_set.len() as u32;
            let amount = pixel_score * self.combo_multiplier;

            self.score += amount;
            self.lines_cleared += num;
            self.clears += num;
            self.level = 1 + self.lines_cleared / 10;

            self.line_clear_cells = clear_set.into_iter().collect();
            self.line_clear_in_progress = true;

            let clear_cells: HashSet<(usize, usize)> =
                self.line_clear_cells.iter().copied().collect();
            self.frozen_grains
                .retain(|fg| !clear_cells.contains(&(fg.x, fg.y)));

            // Score popup for EVERY clear trigger
            let (px, py) = if !self.line_clear_cells.is_empty() {
                self.line_clear_cells[0]
            } else {
                (
                    (self.playfield.width * GRAIN_SCALE) / 2,
                    (self.playfield.height * GRAIN_SCALE) / 2,
                )
            };

            self.popups.push(ScorePopup {
                x: px,
                y: py,
                amount,
                multiplier: self.combo_multiplier,
                age_ms: 0,
                color: Color::Yellow,
            });
        }
    }

    fn spawn_next(&mut self) {
        let width = self.playfield.width as u16;
        let height = self.playfield.height as u16;

        // Pull from queue (kind, color)
        let (next_kind, next_color) = self.next_pieces.remove(0);

        // Add new piece to queue with bias
        let new_kind = self.bag.next();
        let repeat_chance = match self.difficulty {
            crate::Difficulty::Easy => 0.40,
            crate::Difficulty::Medium => 0.20,
            crate::Difficulty::Hard => 0.08,
        };

        // Bias towards the color of the last piece currently in the queue
        let last_queued_color = self.next_pieces.last().map(|(_, c)| *c).unwrap_or(next_color);
        let mut new_color = new_kind.color_index(self.high_color);
        if self.rng.f32() < repeat_chance {
            new_color = last_queued_color;
        }
        self.next_pieces.push((new_kind, new_color));

        self.last_spawn_color = Some(next_color);
        self.piece = Some(Self::spawn_piece(width, height, next_kind, next_color));
        if let Some(ref p) = self.piece {
            self.piece_visual_gx = p.gx as f32;
            self.piece_visual_gy = p.gy as f32;
        }
        if self.spawn_delay_ms > 0 {
            self.spawn_ready_at =
                Some(Instant::now() + std::time::Duration::from_millis(self.spawn_delay_ms));
        } else {
            self.spawn_ready_at = None;
        }
        self.update_game_over_status();
        if !self.playfield.can_place(self.piece.as_ref().unwrap()) {
            self.game_over = true;
        }
    }

    pub fn tick_popups(&mut self, delta_ms: u32) {
        self.popups.retain_mut(|p| {
            let old_steps = p.age_ms / 150;
            p.age_ms += delta_ms;
            let new_steps = p.age_ms / 150;
            if new_steps > old_steps && p.y > 0 {
                p.y = p.y.saturating_sub(1); // Float up smoothly
            }
            p.age_ms < 1500 // Last for 1.5s
        });
    }
}
