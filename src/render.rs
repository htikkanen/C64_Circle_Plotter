use crate::data::*;
use crate::sim::*;

// ============================================================
// Color constants — matching the JS exactly
// ============================================================
const COL_BG: [u8; 3] = [0, 0, 0];
const COL_CHAR: [u8; 3] = [81, 207, 102];
const COL_SPR: [u8; 3] = [255, 107, 107];
const COL_WHITE: [u8; 3] = [219, 219, 219];

const COL_S_CYAN: [u8; 3] = [180, 180, 180];
const COL_S_LTBLUE: [u8; 3] = [104, 174, 255];
const COL_S_PURPLE: [u8; 3] = [100, 130, 180];
const COL_S_BLUE: [u8; 3] = [60, 60, 200];
const COL_S_DKBLUE: [u8; 3] = [20, 20, 120];

/// Dark-to-bright shade ramp for depth-based coloring.
const SHADE_RAMP: [[u8; 3]; 7] = [
    [8, 10, 50],    // darkest
    COL_S_DKBLUE,   // [20, 20, 120]
    COL_S_BLUE,     // [60, 60, 200]
    COL_S_PURPLE,   // [100, 130, 180]
    COL_S_LTBLUE,   // [104, 174, 255]
    COL_S_CYAN,     // [180, 180, 180]
    [219, 219, 219], // white
];

// ============================================================
// Display options
// ============================================================

pub struct DisplayOpts {
    pub grid: bool,
    pub color: bool,
    pub ids: bool,
    pub corruption: bool,
    pub c64only: bool,
    pub mux: bool,
}

impl Default for DisplayOpts {
    fn default() -> Self {
        Self {
            grid: false,
            color: true,
            ids: false,
            corruption: true,
            c64only: true,
            mux: false,
        }
    }
}

// ============================================================
// Frame statistics
// ============================================================

#[derive(Clone)]
pub struct FrameStats {
    pub total: usize,
    pub visible: usize,
    pub sprites: usize,
    pub chars: usize,
    pub conflicts: u32,
    pub max_sl: u8,
    pub mux_overflows: u32,
    pub mux_used: u8,
    pub mem_discs: usize,
    pub on_screen_count: usize,
    pub mem_bytes: usize,
}

// ============================================================
// Disc color — matches JS discColor exactly
// ============================================================

/// Compute the display color for a disc assignment.
///
/// `glitch_color_active`: whether the color-glitch effect is on for this frame.
/// `glitch_frame`: the current frame number (used for glitch seeding).
pub fn disc_color(a: &Assignment, glitch_color_active: bool, glitch_frame: usize) -> [u8; 3] {
    // Color glitch: randomly swap shade index
    if glitch_color_active {
        let gr = ((a.id as u64).wrapping_mul(53).wrapping_add((glitch_frame as u64).wrapping_mul(31))
            & 0x7fff_ffff) % 100;
        if gr < 15 {
            // 15% of discs get wrong color
            let wrong_idx = ((a.id as u64).wrapping_mul(97).wrapping_add((glitch_frame as u64).wrapping_mul(13))
                & 0x7fff_ffff) % (SHADE_RAMP.len() as u64);
            return SHADE_RAMP[wrong_idx as usize];
        }
    }

    let base_idx: usize = if a.is_ghost {
        let d = if a.ghost_depth == 0 { 1 } else { a.ghost_depth };
        if d <= 1 {
            2 // BLUE
        } else if d <= 2 {
            1 // DKBLUE
        } else {
            0 // darkest
        }
    } else {
        let z = a.z;
        if z <= -0.3 {
            5 // CYAN
        } else if z <= -0.1 {
            4 // LTBLUE
        } else if z <= 0.1 {
            3 // PURPLE
        } else if z <= 0.3 {
            2 // BLUE
        } else {
            1 // DKBLUE
        }
    };

    SHADE_RAMP[base_idx.min(SHADE_RAMP.len() - 1)]
}

// ============================================================
// Main rendering entry point
// ============================================================

/// Render a frame into an RGBA pixel buffer (C64W x C64H x 4 bytes).
///
/// Returns:
///   - `Vec<u8>`: the RGBA pixel buffer
///   - `FrameStats`: statistics for the frame
///   - `Vec<u8>`: scanline counts for scanline visualisation (length C64H)
pub fn render_frame(
    frame_positions: &FramePositions,
    spr_pixels: &[u8],
    char_pixels: &[[u8; 64]; 256],
    opts: &DisplayOpts,
) -> (Vec<u8>, FrameStats, Vec<u8>) {
    let positions = &frame_positions.positions;
    // 1. Filter by should_skip
    let vis_positions: Vec<DiscPosition> = positions
        .iter()
        .filter(|p| !should_skip(p.z))
        .cloned()
        .collect();

    // 2. Allocate visible positions
    let alloc_result = allocate(&vis_positions);
    let asgn = &alloc_result.asgn;
    let sl_counts = &alloc_result.sl_counts;
    let sprite_slot_map = &alloc_result.sprite_slot_map;

    // 3. Initialise RGBA buffer to black with full alpha
    let buf_len = C64W * C64H * 4;
    let mut d = vec![0u8; buf_len];
    // Set alpha channel to 255
    for i in (3..buf_len).step_by(4) {
        d[i] = 255;
    }

    // 4. Build screen RAM, owner map, overlap flags, and color RAM
    let cell_count = ROWS * COLS;
    let mut screen_ram = vec![EMPTY_IDX; cell_count];
    let mut screen_owner = vec![-1i32; cell_count];
    let mut screen_over = vec![0u8; cell_count];
    let mut color_ram: Vec<Option<[u8; 3]>> = vec![None; cell_count];

    // Determine glitch state from FramePositions
    let glitch_color_active = frame_positions.glitch_color_active;
    let glitch_frame = frame_positions.glitch_frame;

    // Single char pass: iterate back-to-front
    for (ai, a) in asgn.iter().enumerate() {
        if a.mode != DiscMode::Char {
            continue;
        }
        let col = disc_color(a, glitch_color_active, glitch_frame);
        for cell in &a.stamp {
            let idx = cell.row as usize * COLS + cell.col as usize;
            if idx >= cell_count {
                continue;
            }
            if screen_owner[idx] >= 0 && screen_owner[idx] != ai as i32 {
                screen_over[idx] = screen_over[idx].saturating_add(1);
            }
            screen_ram[idx] = cell.ch;
            screen_owner[idx] = ai as i32;
            color_ram[idx] = Some(col);
        }
    }

    // 5. Build char foreground mask for sprite-background priority
    let mut char_mask = vec![0u8; C64W * C64H];
    for r in 0..ROWS {
        for c in 0..COLS {
            let ch_idx = screen_ram[r * COLS + c];
            if ch_idx == EMPTY_IDX {
                continue;
            }
            let cpx = &char_pixels[ch_idx as usize];
            let bx = c * CHW;
            let by = r * CHH;
            for py in 0..8usize {
                for px in 0..8usize {
                    if cpx[py * 8 + px] != 0 {
                        let sx = bx + px;
                        let sy = by + py;
                        if sx < C64W && sy < C64H {
                            char_mask[sy * C64W + sx] = 1;
                        }
                    }
                }
            }
        }
    }

    // 6. Render sprites
    for (ai, a) in asgn.iter().enumerate() {
        if a.mode != DiscMode::Sprite {
            continue;
        }
        let slot = sprite_slot_map.get(ai).and_then(|s| *s);
        let has_slot = slot.is_some();
        let col: Option<[u8; 3]> = if opts.c64only || !opts.color {
            if has_slot {
                Some(disc_color(a, glitch_color_active, glitch_frame))
            } else {
                None
            }
        } else if has_slot {
            Some(COL_SPR)
        } else {
            Some([100, 30, 30])
        };
        let bg_priority = a.z > 0.0;
        if let Some(col) = col {
            render_sprite(a, &col, bg_priority, spr_pixels, &char_mask, &mut d);
        }
    }

    // 7. Render chars (pass 2) — paints over the pixel buffer
    for r in 0..ROWS {
        for c in 0..COLS {
            let ch_idx = screen_ram[r * COLS + c];
            let cpx = &char_pixels[ch_idx as usize];
            let bx = c * CHW;
            let by = r * CHH;
            let was_over = screen_over[r * COLS + c] > 0;

            let cell_color = color_ram[r * COLS + c];

            let (fg, bg): ([u8; 3], [u8; 3]) = if opts.c64only {
                (cell_color.unwrap_or(COL_WHITE), COL_BG)
            } else if opts.color && was_over && opts.corruption {
                ([255, 212, 59], [60, 40, 0])
            } else if opts.color {
                (COL_CHAR, COL_BG)
            } else {
                (COL_WHITE, COL_BG)
            };

            let show_empty = opts.grid && !opts.c64only;
            let empty_bg: [u8; 3] = [40, 20, 60];

            for py in 0..8usize {
                for px in 0..8usize {
                    let sx = bx + px;
                    let sy = by + py;
                    if sx < C64W && sy < C64H {
                        let idx = (sy * C64W + sx) * 4;
                        if cpx[py * 8 + px] != 0 {
                            d[idx] = fg[0];
                            d[idx + 1] = fg[1];
                            d[idx + 2] = fg[2];
                        } else if was_over && opts.corruption && !opts.c64only {
                            d[idx] = bg[0];
                            d[idx + 1] = bg[1];
                            d[idx + 2] = bg[2];
                        } else if show_empty && ch_idx != EMPTY_IDX {
                            d[idx] = empty_bg[0];
                            d[idx + 1] = empty_bg[1];
                            d[idx + 2] = empty_bg[2];
                        }
                    }
                }
            }
        }
    }

    // 8. Compute memory stats (pixel-level occlusion)
    let mut mem_positions: Vec<&DiscPosition> = positions
        .iter()
        .filter(|p| {
            let ox = p.x.round() as i32 - 8;
            let oy = p.y.round() as i32 - 8;
            ox + 16 > 0 && ox < C64W as i32 && oy + 15 > 0 && oy < C64H as i32
        })
        .collect();
    // Sort front-to-back (highest z first)
    mem_positions.sort_by(|a, b| b.z.partial_cmp(&a.z).unwrap_or(std::cmp::Ordering::Equal));

    let mut screen_claimed = vec![0u8; C64W * C64H];
    let mut mem_discs: usize = 0;
    let on_screen_count = mem_positions.len();

    for p in &mem_positions {
        let ox = p.x.round() as i32 - 8;
        let oy = p.y.round() as i32 - 8;
        let mut new_pixels: u32 = 0;
        for sr in 0..SPRITE_H {
            let sy = oy + sr as i32;
            if sy < 0 || sy >= C64H as i32 {
                continue;
            }
            for sc in 0..SPRITE_W {
                if spr_pixels[sr * SPRITE_W + sc] == 0 {
                    continue;
                }
                let sx = ox + sc as i32;
                if sx < 0 || sx >= C64W as i32 {
                    continue;
                }
                let idx = sy as usize * C64W + sx as usize;
                if screen_claimed[idx] == 0 {
                    screen_claimed[idx] = 1;
                    new_pixels += 1;
                }
            }
        }
        if new_pixels > 0 {
            mem_discs += 1;
        }
    }

    let mem_bytes = mem_discs * 3;

    // 9. Compute stats
    let sprite_count = asgn.iter().filter(|a| a.mode == DiscMode::Sprite).count();
    let char_count = asgn.iter().filter(|a| a.mode == DiscMode::Char).count();
    let visible_count = asgn
        .iter()
        .filter(|a| a.mode != DiscMode::Offscreen)
        .count();

    let stats = FrameStats {
        total: asgn.len(),
        visible: visible_count,
        sprites: sprite_count,
        chars: char_count,
        conflicts: alloc_result.conflicts,
        max_sl: alloc_result.max_sl,
        mux_overflows: alloc_result.mux_overflows,
        mux_used: alloc_result.mux_used,
        mem_discs,
        on_screen_count,
        mem_bytes,
    };

    (d, stats, sl_counts.to_vec())
}

// ============================================================
// Helper: render a single sprite into the pixel buffer
// ============================================================

fn render_sprite(
    a: &Assignment,
    col: &[u8; 3],
    bg_priority: bool,
    spr_pixels: &[u8],
    char_mask: &[u8],
    d: &mut [u8],
) {
    let ox = a.x.floor() as i32 - 8;
    let oy = a.y.floor() as i32 - 8;
    for sr in 0..SPRITE_H {
        let sy = oy + sr as i32;
        if sy < 0 || sy >= C64H as i32 {
            continue;
        }
        for sc in 0..SPRITE_W {
            if spr_pixels[sr * SPRITE_W + sc] == 0 {
                continue;
            }
            let sx = ox + sc as i32;
            if sx < 0 || sx >= C64W as i32 {
                continue;
            }
            // Background priority: char fg pixels appear in front of sprite
            if bg_priority && char_mask[sy as usize * C64W + sx as usize] != 0 {
                continue;
            }
            let idx = (sy as usize * C64W + sx as usize) * 4;
            d[idx] = col[0];
            d[idx + 1] = col[1];
            d[idx + 2] = col[2];
        }
    }
}
