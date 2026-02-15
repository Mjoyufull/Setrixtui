//! Autoplay bot for Setrixtui.
//!
//! Strategy: place same-color pieces near existing same-color sand to build
//! horizontal color bands that can span left-to-right for clears.
//! Uses a lightweight grid snapshot — never clones GameState.

use crate::game::{Cell, GameState, GRAIN_SCALE};
use crate::input::Action;
use std::collections::{HashSet, VecDeque};


pub struct Bot;

#[derive(Debug, Clone)]
struct MoveCandidate {
    score: f32,
    rotation: u8,
    target_gx: i32,
    initial_gx: i32,
}

impl Bot {
    /// Evaluate all reachable (rotation, column) placements for the current piece.
    /// Returns a sequence of actions (rotations, moves, hard-drop) for the best one.
    pub fn find_best_move(
        state: &GameState,
    ) -> VecDeque<Action> {
        let piece = match state.piece {
            Some(ref p) => p,
            None => return VecDeque::new(),
        };

        let (gw, gh) = state.playfield.grain_dims();
        let initial_gx = piece.gx;
        let piece_color = piece.kind.color_index(state.high_color);

        // Snapshot the grid: 0=empty, color+1=occupied.
        let base_grid = snapshot_grid(&state.playfield, &state.frozen_grains, gw, gh);

        let mut best = MoveCandidate {
            score: f32::NEG_INFINITY,
            rotation: 0,
            target_gx: initial_gx,
            initial_gx,
        };

        // Column step range (in block units) relative to current piece.
        let blocks_wide = gw as i32 / GRAIN_SCALE as i32;
        let min_step = -blocks_wide - 1;
        let max_step = blocks_wide + 1;

        for r in 0..4u8 {
            let mut test_piece = piece.clone();
            test_piece.rotation = r;

            for step in min_step..=max_step {
                let target_gx = initial_gx + step * GRAIN_SCALE as i32;

                // Quick bounds reject.
                if target_gx < -(GRAIN_SCALE as i32 * 2)
                    || target_gx > gw as i32 + GRAIN_SCALE as i32
                {
                    continue;
                }

                test_piece.gx = target_gx;
                test_piece.gy = 0;

                // Feasibility at spawn height.
                if !state
                    .playfield
                    .can_place_with_frozen(&test_piece, &state.frozen_grains)
                {
                    continue;
                }

                // Hard-drop: find landing Y via grid collision.
                let mut land_y = test_piece.gy;
                loop {
                    test_piece.gy = land_y + 1;
                    if !can_place_on_grid(&base_grid, gw, gh, &test_piece) {
                        break;
                    }
                    land_y += 1;
                }
                test_piece.gy = land_y;

                // Clone grid, stamp piece, run simplified settle.
                let mut grid = base_grid.clone();
                let stamp_val = piece_color + 1; // grid uses color+1
                stamp_piece(&mut grid, gw, gh, &test_piece, stamp_val);
                settle_sand(&mut grid, gw, gh);

                // Evaluate the resulting board.
                let score = evaluate(&grid, gw, gh, piece_color);

                if score > best.score {
                    best = MoveCandidate {
                        score,
                        rotation: r,
                        target_gx: target_gx,
                        initial_gx,
                    };
                }
            }
        }

        // Build action sequence.
        build_actions(piece.rotation, best.rotation, best.initial_gx, best.target_gx)
    }
}

/// Build the action queue: rotations → lateral moves → hard-drop.
fn build_actions(cur_rot: u8, target_rot: u8, cur_gx: i32, target_gx: i32) -> VecDeque<Action> {
    let mut actions = VecDeque::new();

    // Rotations (always CW for simplicity).
    let mut r = cur_rot;
    while r != target_rot {
        actions.push_back(Action::RotateCw);
        r = (r + 1) % 4;
    }

    // Lateral movement.
    let diff = target_gx - cur_gx;
    let steps = diff / GRAIN_SCALE as i32;
    if steps < 0 {
        for _ in 0..steps.abs() {
            actions.push_back(Action::MoveLeft);
        }
    } else {
        for _ in 0..steps {
            actions.push_back(Action::MoveRight);
        }
    }

    actions.push_back(Action::HardDrop);
    actions
}

// ---------------------------------------------------------------------------
// Grid helpers
// ---------------------------------------------------------------------------

/// Snapshot playfield + frozen grains into a flat grid. 0=empty, color+1=sand.
fn snapshot_grid(
    pf: &crate::game::Playfield,
    frozen: &[crate::game::FrozenGrain],
    gw: usize,
    gh: usize,
) -> Vec<u8> {
    let mut grid = vec![0u8; gw * gh];
    for y in 0..gh {
        for x in 0..gw {
            if let Some(Cell::Sand(c, _)) = pf.get(x, y) {
                grid[y * gw + x] = c + 1;
            }
        }
    }
    for fg in frozen {
        if fg.x < gw && fg.y < gh {
            grid[fg.y * gw + fg.x] = fg.color_index + 1;
        }
    }
    grid
}

/// Check if a piece fits on the flat grid.
fn can_place_on_grid(grid: &[u8], gw: usize, gh: usize, piece: &crate::game::Piece) -> bool {
    for (gx_o, gy_o) in piece.cell_grain_origins() {
        for dy in 0..GRAIN_SCALE as i32 {
            for dx in 0..GRAIN_SCALE as i32 {
                let gx = gx_o + dx;
                let gy = gy_o + dy;
                if gx < 0 || gx >= gw as i32 || gy >= gh as i32 {
                    return false;
                }
                if gy < 0 {
                    continue;
                }
                if grid[gy as usize * gw + gx as usize] != 0 {
                    return false;
                }
            }
        }
    }
    true
}

/// Stamp piece grains onto the grid.
fn stamp_piece(grid: &mut [u8], gw: usize, gh: usize, piece: &crate::game::Piece, color: u8) {
    for (gx_o, gy_o) in piece.cell_grain_origins() {
        for dy in 0..GRAIN_SCALE as i32 {
            for dx in 0..GRAIN_SCALE as i32 {
                let gx = gx_o + dx;
                let gy = gy_o + dy;
                if gx >= 0 && gx < gw as i32 && gy >= 0 && gy < gh as i32 {
                    grid[gy as usize * gw + gx as usize] = color;
                }
            }
        }
    }
}

/// Simplified sand settling: grains fall straight down or cascade diagonally.
fn settle_sand(grid: &mut [u8], gw: usize, gh: usize) {
    for pass in 0..80 {
        let mut moved = false;
        let left_first = pass % 2 == 0;

        for y in (0..gh.saturating_sub(1)).rev() {
            for x in 0..gw {
                let idx = y * gw + x;
                let c = grid[idx];
                if c == 0 {
                    continue;
                }
                let below = (y + 1) * gw + x;
                if grid[below] == 0 {
                    grid[below] = c;
                    grid[idx] = 0;
                    moved = true;
                } else {
                    let can_left = x > 0 && grid[(y + 1) * gw + x - 1] == 0;
                    let can_right = x + 1 < gw && grid[(y + 1) * gw + x + 1] == 0;
                    let go_left = if can_left && can_right { left_first } else { can_left };
                    if go_left {
                        grid[(y + 1) * gw + x - 1] = c;
                        grid[idx] = 0;
                        moved = true;
                    } else if can_right {
                        grid[(y + 1) * gw + x + 1] = c;
                        grid[idx] = 0;
                        moved = true;
                    }
                }
            }
        }
        if !moved {
            break;
        }
    }
}

// ---------------------------------------------------------------------------
// Board evaluation — the core brain of the bot
// ---------------------------------------------------------------------------

/// Score a settled board. Higher = better.
fn evaluate(
    grid: &[u8],
    gw: usize,
    gh: usize,
    placed_color: u8,
) -> f32 {
    let mut score: f32 = 0.0;
    let placed_val = placed_color + 1;

    // --- Scoring Constants ---
    const W_SPAN_CLEAR: f32 = 50.0;
    const W_HOLES: f32 = 8.0;
    const W_MAX_HEIGHT: f32 = 3.5;
    const W_AGG_HEIGHT: f32 = 0.15;
    const W_BUMPINESS: f32 = 1.5;
    const W_H_ADJACENCY: f32 = 0.8;
    const W_V_ADJACENCY: f32 = 0.2;
    const W_PROXIMITY: f32 = 0.5;
    const W_REACH: f32 = 5.0;
    const W_DANGER: f32 = 100.0;

    // --- 1. Spanning clears (instant massive reward) ---
    let clears = count_spanning_clears(grid, gw, gh);
    score += clears as f32 * W_SPAN_CLEAR;

    // --- 2. Column heights, holes, bumpiness ---
    let mut col_heights = vec![0usize; gw];
    let mut holes: u32 = 0;

    for x in 0..gw {
        let mut found_top = false;
        for y in 0..gh {
            if grid[y * gw + x] != 0 {
                if !found_top {
                    col_heights[x] = gh - y;
                    found_top = true;
                }
            } else if found_top {
                holes += 1;
            }
        }
    }

    let max_height = *col_heights.iter().max().unwrap_or(&0);
    let agg_height: usize = col_heights.iter().sum();
    let bumpiness: i32 = col_heights
        .windows(2)
        .map(|w| (w[0] as i32 - w[1] as i32).abs())
        .sum();

    score -= holes as f32 * W_HOLES;
    score -= max_height as f32 * W_MAX_HEIGHT;
    score -= agg_height as f32 * W_AGG_HEIGHT;
    score -= bumpiness as f32 * W_BUMPINESS;

    // --- 3. Same-color adjacency (horizontal only — we want horizontal bands) ---
    // Horizontal adjacency matters much more than vertical for spanning paths.
    let mut h_adj: u32 = 0;
    let mut v_adj: u32 = 0;
    for y in 0..gh {
        for x in 0..gw {
            let c = grid[y * gw + x];
            if c != 0 {
                if x + 1 < gw && grid[y * gw + x + 1] == c {
                    h_adj += 1;
                }
                if y + 1 < gh && grid[(y + 1) * gw + x] == c {
                    v_adj += 1;
                }
            }
        }
    }
    // Horizontal adjacency is king for spanning paths.
    score += h_adj as f32 * W_H_ADJACENCY;
    score += v_adj as f32 * W_V_ADJACENCY;

    // --- 4. Placed piece proximity to same-color sand ---
    // Reward placing near existing sand of the same color.
    // This encourages color clustering which leads to spans.
    let proximity_bonus = same_color_proximity(grid, gw, gh, placed_val);
    score += proximity_bonus * W_PROXIMITY;

    // --- 5. Horizontal reach per color (how close each color is to spanning) ---
    let reach_bonus = color_reach_bonus(grid, gw, gh, W_REACH);
    score += reach_bonus;

    // --- 6. Danger zone ---
    if max_height > gh.saturating_sub(10) {
        score -= W_DANGER;
    }

    score
}

/// Count same-color 8-connected components that span left-to-right.
fn count_spanning_clears(grid: &[u8], gw: usize, gh: usize) -> u32 {
    let mut clears = 0u32;
    let mut visited = HashSet::new();

    for color in 1..=6u8 {
        for start_y in 0..gh {
            let pos = (0usize, start_y);
            if grid[start_y * gw] != color || visited.contains(&pos) {
                continue;
            }
            let mut stack = vec![pos];
            visited.insert(pos);
            let mut touches_right = false;

            while let Some((x, y)) = stack.pop() {
                if x == gw - 1 {
                    touches_right = true;
                }
                for &(dx, dy) in &NEIGHBOURS_8 {
                    let nx = x as i32 + dx;
                    let ny = y as i32 + dy;
                    if nx >= 0 && nx < gw as i32 && ny >= 0 && ny < gh as i32 {
                        let npos = (nx as usize, ny as usize);
                        if grid[npos.1 * gw + npos.0] == color && !visited.contains(&npos) {
                            visited.insert(npos);
                            stack.push(npos);
                        }
                    }
                }
            }
            if touches_right {
                clears += 1;
            }
        }
    }
    clears
}

const NEIGHBOURS_8: [(i32, i32); 8] = [
    (-1, -1), (-1, 0), (-1, 1),
    (0, -1),           (0, 1),
    (1, -1),  (1, 0),  (1, 1),
];

/// How many grains of the placed color are adjacent to other grains of the same color?
/// Measures clustering — higher = piece landed in a good color neighbourhood.
fn same_color_proximity(grid: &[u8], gw: usize, gh: usize, color_val: u8) -> f32 {
    let mut count: f32 = 0.0;
    for y in 0..gh {
        for x in 0..gw {
            if grid[y * gw + x] != color_val {
                continue;
            }
            // Check horizontal neighbours only (horizontal bands matter most).
            if x > 0 && grid[y * gw + x - 1] == color_val {
                count += 1.0;
            }
            if x + 1 < gw && grid[y * gw + x + 1] == color_val {
                count += 1.0;
            }
        }
    }
    count
}

/// Bonus for how far each color reaches across the board (max_x - min_x).
/// Colors spanning >50% width get a proportional bonus.
fn color_reach_bonus(grid: &[u8], gw: usize, gh: usize, base_reach: f32) -> f32 {
    let mut bonus: f32 = 0.0;
    let edge_bonus = base_reach * 0.75;
    let touch_bonus = base_reach * 0.125;

    for color in 1..=6u8 {
        let mut min_x = gw;
        let mut max_x = 0usize;
        let mut count = 0u32;

        for y in 0..gh {
            for x in 0..gw {
                if grid[y * gw + x] == color {
                    if x < min_x { min_x = x; }
                    if x > max_x { max_x = x; }
                    count += 1;
                }
            }
        }

        if count == 0 || min_x >= max_x {
            continue;
        }

        let reach = max_x - min_x;
        let ratio = reach as f32 / gw as f32;

        // Bonus scales sharply above 50% reach.
        if ratio > 0.5 {
            bonus += (ratio - 0.5) * base_reach;
        }
        // Extra bonus if touching both edges.
        if min_x == 0 && max_x == gw - 1 {
            bonus += edge_bonus;
        }
        // Bonus for touching left edge.
        if min_x == 0 {
            bonus += ratio * touch_bonus;
        }
        // Bonus for touching right edge.
        if max_x == gw - 1 {
            bonus += ratio * touch_bonus;
        }
    }
    bonus
}
