# Setrixtui

Terminal puzzle game: Setris/Sandtrix-style falling blocks that turn into sand. You clear lines by making a single colour connect the **left edge to the right edge** (8-neighbour, path can be diagonal). Matching piece colours and completing those spans scores points; remaining sand falls under gravity.

## Requirements

- Rust 1.86+ (ratatui 0.30)
- Edition 2024

## Installation
```bash
cargo install setrixtui
```

```bash
cargo build --release
./target/release/setrixtui
```

## Running

```bash
./target/release/setrixtui
```

Default: main menu (difficulty, mode), then play. Endless mode, easy difficulty, One Dark theme from `onedark.theme` if you pass `--theme ./onedark.theme` (otherwise built-in One Dark).

```bash
./target/release/setrixtui --no-menu
```

Starts the game immediately, no menu.

```bash
./target/release/setrixtui --width 10 --height 24 --theme ./onedark.theme
./target/release/setrixtui -m timed --time-limit 180 -d hard --no-animation
```

Playfield size is in **grid cells** (columns and rows). Default is 10 columns and 24 rows. If your terminal is too small for that, the game **clamps** width and height so the whole board plus border and sidebar fit on screen. So e.g. `--height 50` on a 64-row terminal will use a smaller height that fits (border and sidebar included); the bottom is no longer drawn off-screen.

Game over: **R** restart, **Q** quit.

## Layout

- **Playfield**: left side, bordered. Each block is 6×6 “grains”; the board is drawn with half-blocks (▀) so two grain rows per terminal row.
- **Sidebar** (24 cols): next-piece preview (real shape), the six sand colours, score, level, and (in timed mode) remaining time.

Exact size: playfield needs `(width*6 + 2)` columns and `(height*3 + 2)` rows (border counts); plus 24 columns for the sidebar. If the terminal is smaller, the used playfield size is reduced so everything fits.

## Modes

- **Endless** (default): play until stack overflow. **R** to restart, **Q** to quit.
- **Timed** (`-m timed`, `--time-limit SECS`): score as much as you can before time runs out. When time is up you see “Time’s up!” and can **R** or **Q**.
- **Clear** (`-m clear`, `--clear-lines N`): win by clearing N edge-to-edge lines. “You win!” at the target.

## Controls

Movement keys repeat if held. Normal and vim-style both work.

| Action     | Normal        | Vim    |
|------------|---------------|--------|
| Left       | ←             | h      |
| Right      | →             | l      |
| Rotate CW  | ↑             | k / i  |
| Rotate CCW | —             | u      |
| Soft drop  | ↓             | j      |
| Hard drop  | Enter / Space | Space  |
| Pause      | p             | p      |
| Quit       | q / Esc       | q      |

**P** toggles pause. On game over / win / time’s up: **R** restart, **Q** quit.

## Theme and colours

Themes are btop-style: `theme[key]="value"` with hex colours. Example: `onedark.theme` in the repo.

- **With `--theme FILE`**: colours are read from the file. Sand colours map from keys like `mem_box`, `title`, `cpu_end`, `cpu_box`, `net_box`, `hi_fg`; UI from `meter_bg`, `div_line`, `main_fg`, `title`, `inactive_fg`. No extra saturation: the hex values from the file are used as-is.
- **Without a theme file**: built-in One Dark is used, with the same hex values as in `onedark.theme` (e.g. `#98C379`, `#E5C07B`, `#E06C75`, `#61AFEF`, `#C678DD`, `#56B6C2` for the six sand colours; `#31353F` background; `#3F444F` dividers).

`--palette high-contrast` or `--palette colorblind` overrides only the **sand** colours (for visibility); they do not change the rest of the theme.

## CLI options (summary)

- **Playfield**: `--width COLS`, `--height ROWS` (default 10×24). Clamped to terminal size so the window fits.
- **Mode**: `-m endless|timed|clear`. Timed: `--time-limit SECS`. Clear: `--clear-lines N`.
- **Difficulty**: `-d easy|medium|hard` (affects gravity and, if applicable, width).
- **Theme**: `--theme FILE` (btop-style). `--palette normal|high-contrast|colorblind` for sand only.
- **Tuning**: `--tick-rate`, `--frame-rate`, `--spawn-delay-ms`, `--lock-delay-ms`, `--initial-level`, `--relaxed`, `--sand-settle`, `--no-animation`, `--no-menu`, `--high-color`.

Full list: `setrixtui --help`.
