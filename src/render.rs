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
    pub show_chars: bool,
    pub show_sprites: bool,
    pub prune_dist: f64,
    pub error_overlay: bool,
    pub ideal_render: bool,
    pub mux_overlay: bool,
}

impl Default for DisplayOpts {
    fn default() -> Self {
        Self {
            grid: false,
            color: true,
            ids: false,
            corruption: true,
            c64only: true,
            show_chars: true,
            show_sprites: true,
            prune_dist: 2.0,
            error_overlay: false,
            ideal_render: false,
            mux_overlay: false,
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
    pub pixel_error: u32,
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
// Standalone render functions for optimizer scoring
// ============================================================

/// Render ideal image: all discs as perfect circles, back-to-front, no C64 constraints.
/// Returns RGB buffer (C64W * C64H * 3).
pub fn render_ideal(
    vis_positions: &[DiscPosition],
    glitch_color_active: bool,
    glitch_frame: usize,
) -> Vec<u8> {
    let spr_pixels = &*SPR_PIXELS;
    let mut buf = vec![0u8; C64W * C64H * 3];

    // Sort by effective z descending (back first, front paints last = on top)
    let mut sorted: Vec<&DiscPosition> = vis_positions.iter().collect();
    sorted.sort_by(|a, b| {
        let za = if a.is_ghost { a.z + 10.0 + a.ghost_depth as f64 } else { a.z };
        let zb = if b.is_ghost { b.z + 10.0 + b.ghost_depth as f64 } else { b.z };
        zb.total_cmp(&za)
    });

    for p in &sorted {
        let col = {
            let a = Assignment {
                x: p.x, y: p.y, z: p.z, id: p.id,
                is_ghost: p.is_ghost, ghost_depth: p.ghost_depth,
                mode: DiscMode::Char, stamp: Vec::new(),
            };
            disc_color(&a, glitch_color_active, glitch_frame)
        };
        let ox = p.x.floor() as i32 - 8;
        let oy = p.y.floor() as i32 - 8;
        for sr in 0..SPRITE_H {
            let sy = oy + sr as i32;
            if sy < 0 || sy >= C64H as i32 { continue; }
            for sc in 0..SPRITE_W {
                if spr_pixels[sr * SPRITE_W + sc] == 0 { continue; }
                let sx = ox + sc as i32;
                if sx < 0 || sx >= C64W as i32 { continue; }
                let idx = (sy as usize * C64W + sx as usize) * 3;
                buf[idx] = col[0];
                buf[idx + 1] = col[1];
                buf[idx + 2] = col[2];
            }
        }
    }
    buf
}

/// Render C64-accurate image from assignments. Returns RGB buffer (C64W * C64H * 3).
pub fn render_c64_image(
    asgn: &[Assignment],
    sprite_slot_map: &[Option<u8>],
    glitch_color_active: bool,
    glitch_frame: usize,
) -> Vec<u8> {
    let spr_pixels = &*SPR_PIXELS;
    let char_pixels = &*CHAR_PIXELS;
    let cell_count = ROWS * COLS;
    let mut screen_ram = vec![EMPTY_IDX as u16; cell_count];
    let mut color_ram: Vec<Option<[u8; 3]>> = vec![None; cell_count];

    // Char stamping: ghosts back-to-front, then non-ghosts back-to-front
    for ai in (0..asgn.len()).rev() {
        let a = &asgn[ai];
        if a.mode != DiscMode::Char || !a.is_ghost { continue; }
        let col = disc_color(a, glitch_color_active, glitch_frame);
        for cell in &a.stamp {
            let idx = cell.row as usize * COLS + cell.col as usize;
            if idx >= cell_count { continue; }
            screen_ram[idx] = cell.ch;
            color_ram[idx] = Some(col);
        }
    }
    for ai in (0..asgn.len()).rev() {
        let a = &asgn[ai];
        if a.mode != DiscMode::Char || a.is_ghost { continue; }
        let col = disc_color(a, glitch_color_active, glitch_frame);
        for cell in &a.stamp {
            let idx = cell.row as usize * COLS + cell.col as usize;
            if idx >= cell_count { continue; }
            screen_ram[idx] = cell.ch;
            color_ram[idx] = Some(col);
        }
    }

    let mut buf = vec![0u8; C64W * C64H * 3];

    // Paint ghost sprites (background, $D01B=1, behind chars)
    for slot in (0..8u8).rev() {
        for (ai, a) in asgn.iter().enumerate() {
            if a.mode != DiscMode::Sprite || !a.is_ghost { continue; }
            if sprite_slot_map.get(ai).and_then(|s| *s) != Some(slot) { continue; }
            let col = disc_color(a, glitch_color_active, glitch_frame);
            let ox = a.x.floor() as i32 - 8;
            let oy = a.y.floor() as i32 - 8;
            for sr in 0..SPRITE_H {
                let sy = oy + sr as i32;
                if sy < 0 || sy >= C64H as i32 { continue; }
                for sc in 0..SPRITE_W {
                    if spr_pixels[sr * SPRITE_W + sc] == 0 { continue; }
                    let sx = ox + sc as i32;
                    if sx < 0 || sx >= C64W as i32 { continue; }
                    let idx = (sy as usize * C64W + sx as usize) * 3;
                    buf[idx] = col[0];
                    buf[idx + 1] = col[1];
                    buf[idx + 2] = col[2];
                }
            }
        }
    }

    // Paint chars to buffer
    for r in 0..ROWS {
        for c in 0..COLS {
            let ch_idx = screen_ram[r * COLS + c];
            let cpx = &char_pixels[ch_idx as usize];
            let fg = color_ram[r * COLS + c].unwrap_or([0, 0, 0]);
            let bx = c * CHW;
            let by = r * CHH;
            for py in 0..8usize {
                for px in 0..8usize {
                    if cpx[py * 8 + px] != 0 {
                        let sx = bx + px;
                        let sy = by + py;
                        if sx < C64W && sy < C64H {
                            let idx = (sy * C64W + sx) * 3;
                            buf[idx] = fg[0];
                            buf[idx + 1] = fg[1];
                            buf[idx + 2] = fg[2];
                        }
                    }
                }
            }
        }
    }

    // Paint non-ghost sprites on top (foreground, $D01B = 0)
    // Reverse slot order: slot 7 first → slot 0 last (highest priority on top)
    for slot in (0..8u8).rev() {
        for (ai, a) in asgn.iter().enumerate() {
            if a.mode != DiscMode::Sprite || a.is_ghost { continue; }
            if sprite_slot_map.get(ai).and_then(|s| *s) != Some(slot) { continue; }
            let col = disc_color(a, glitch_color_active, glitch_frame);
            let ox = a.x.floor() as i32 - 8;
            let oy = a.y.floor() as i32 - 8;
            for sr in 0..SPRITE_H {
                let sy = oy + sr as i32;
                if sy < 0 || sy >= C64H as i32 { continue; }
                for sc in 0..SPRITE_W {
                    if spr_pixels[sr * SPRITE_W + sc] == 0 { continue; }
                    let sx = ox + sc as i32;
                    if sx < 0 || sx >= C64W as i32 { continue; }
                    let idx = (sy as usize * C64W + sx as usize) * 3;
                    buf[idx] = col[0];
                    buf[idx + 1] = col[1];
                    buf[idx + 2] = col[2];
                }
            }
        }
    }
    buf
}

/// Count pixels that differ between two RGB buffers.
pub fn pixel_error(actual: &[u8], ideal: &[u8]) -> u32 {
    actual.chunks_exact(3).zip(ideal.chunks_exact(3))
        .filter(|(a, i)| a[0] != i[0] || a[1] != i[1] || a[2] != i[2])
        .count() as u32
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
    opts: &DisplayOpts,
    override_alloc: Option<(&[Assignment], &[Option<u8>])>,
) -> (Vec<u8>, FrameStats, Vec<u8>) {
    let spr_pixels = &*SPR_PIXELS;
    let char_pixels = &*CHAR_PIXELS;
    let positions = &frame_positions.positions;
    // 1. Filter by should_skip
    let mut vis_positions: Vec<DiscPosition> = positions
        .iter()
        .filter(|p| !should_skip(p.z))
        .cloned()
        .collect();

    // 1b. Proximity pruning
    prune_by_proximity(&mut vis_positions, opts.prune_dist);

    // 2. Allocate visible positions (or use optimizer override)
    let (asgn, sl_counts, sprite_slot_map, max_sl, mux_overflows, mux_used, conflicts);
    if let Some((ov_asgn, ov_slots)) = override_alloc {
        asgn = ov_asgn.to_vec();
        sprite_slot_map = ov_slots.to_vec();
        // Recompute stats from override
        let mut sl = vec![0u8; C64H];
        for (ai, a) in asgn.iter().enumerate() {
            if a.mode != DiscMode::Sprite { continue; }
            if ov_slots.get(ai).and_then(|s| *s).is_none() { continue; }
            let top = ((a.y.floor() as i32) - 8).max(0) as usize;
            let bot = ((a.y.floor() as i32) - 8 + SPRITE_H as i32 - 1).min(C64H as i32 - 1) as usize;
            for s in top..=bot { sl[s] += 1; }
        }
        max_sl = *sl.iter().max().unwrap_or(&0);
        sl_counts = sl;
        mux_overflows = 0;
        mux_used = asgn.iter().enumerate()
            .filter(|(i, a)| a.mode == DiscMode::Sprite && ov_slots.get(*i).and_then(|s| *s).is_some())
            .filter_map(|(i, _)| ov_slots[i])
            .max().map(|s| s + 1).unwrap_or(0);
        conflicts = 0; // optimizer already minimized these
    } else {
        let result = allocate(&vis_positions);
        asgn = result.asgn;
        sl_counts = result.sl_counts;
        sprite_slot_map = result.sprite_slot_map;
        max_sl = result.max_sl;
        mux_overflows = result.mux_overflows;
        mux_used = result.mux_used;
        conflicts = result.conflicts;
    }

    // 3. Initialise RGBA buffer to black with full alpha
    let buf_len = C64W * C64H * 4;
    let mut d = vec![0u8; buf_len];
    // Set alpha channel to 255
    for i in (3..buf_len).step_by(4) {
        d[i] = 255;
    }

    // 4. Build screen RAM, owner map, overlap flags, and color RAM
    let cell_count = ROWS * COLS;
    let mut screen_ram = vec![EMPTY_IDX as u16; cell_count];
    let mut screen_owner = vec![-1i32; cell_count];
    let mut screen_over = vec![0u8; cell_count];
    let mut color_ram: Vec<Option<[u8; 3]>> = vec![None; cell_count];

    // Determine glitch state from FramePositions
    let glitch_color_active = frame_positions.glitch_color_active;
    let glitch_frame = frame_positions.glitch_frame;

    // 4a. Render ghost sprites — background priority ($D01B=1), behind chars
    //     Mux IRQ sets $D01B per instance, so this is C64 compatible.
    if opts.show_sprites {
        for slot in (0..8u8).rev() {
            for (ai, a) in asgn.iter().enumerate() {
                if a.mode != DiscMode::Sprite || !a.is_ghost { continue; }
                if sprite_slot_map.get(ai).and_then(|s| *s) != Some(slot) { continue; }
                let col = disc_color(a, glitch_color_active, glitch_frame);
                let ox = a.x.floor() as i32 - 8;
                let oy = a.y.floor() as i32 - 8;
                for sr in 0..SPRITE_H {
                    let sy = oy + sr as i32;
                    if sy < 0 || sy >= C64H as i32 { continue; }
                    for sc in 0..SPRITE_W {
                        if spr_pixels[sr * SPRITE_W + sc] == 0 { continue; }
                        let sx = ox + sc as i32;
                        if sx < 0 || sx >= C64W as i32 { continue; }
                        let idx = (sy as usize * C64W + sx as usize) * 4;
                        set_rgb(&mut d, idx, col);
                    }
                }
            }
        }
    }

    // Two-pass char stamping: ghosts first, then main discs overwrite.
    // C64 compatible: all stamp cells are written, last writer wins.
    // Iterate back-to-front (reverse array order) so front discs write last = on top.
    let mut non_ghost_owner = vec![-1i32; cell_count];

    // Pass 1: ghost discs (background layer, back-to-front)
    for ai in (0..asgn.len()).rev() {
        let a = &asgn[ai];
        if a.mode != DiscMode::Char || !a.is_ghost {
            continue;
        }
        let col = disc_color(a, glitch_color_active, glitch_frame);
        for cell in &a.stamp {
            let idx = cell.row as usize * COLS + cell.col as usize;
            if idx >= cell_count { continue; }
            screen_ram[idx] = cell.ch;
            screen_owner[idx] = ai as i32;
            color_ram[idx] = Some(col);
        }
    }
    // Pass 2: non-ghost discs (overwrite ghosts, back-to-front)
    for ai in (0..asgn.len()).rev() {
        let a = &asgn[ai];
        if a.mode != DiscMode::Char || a.is_ghost {
            continue;
        }
        let col = disc_color(a, glitch_color_active, glitch_frame);
        for cell in &a.stamp {
            let idx = cell.row as usize * COLS + cell.col as usize;
            if idx >= cell_count { continue; }
            if non_ghost_owner[idx] >= 0 && non_ghost_owner[idx] != ai as i32 {
                screen_over[idx] = screen_over[idx].saturating_add(1);
            }
            screen_ram[idx] = cell.ch;
            screen_owner[idx] = ai as i32;
            non_ghost_owner[idx] = ai as i32;
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

    // Helper: determine sprite display color
    let sprite_col = |a: &Assignment, ai: usize| -> Option<[u8; 3]> {
        let has_slot = sprite_slot_map.get(ai).and_then(|s| *s).is_some();
        if opts.c64only || !opts.color {
            if has_slot { Some(disc_color(a, glitch_color_active, glitch_frame)) } else { None }
        } else if has_slot {
            Some(COL_SPR)
        } else {
            Some([100, 30, 30])
        }
    };

    // 6. Render chars
    if opts.show_chars {
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
                                set_rgb(&mut d, idx, fg);
                            } else if was_over && opts.corruption && !opts.c64only {
                                set_rgb(&mut d, idx, bg);
                            } else if show_empty && ch_idx != EMPTY_IDX {
                                set_rgb(&mut d, idx, empty_bg);
                            }
                        }
                    }
                }
            }
        }
    }

    // 7. Render non-ghost sprites — foreground priority ($D01B = 0)
    //    Reverse slot order: slot 7 first → slot 0 last (highest priority on top)
    if opts.show_sprites {
        for slot in (0..8u8).rev() {
            for (ai, a) in asgn.iter().enumerate() {
                if a.mode != DiscMode::Sprite || a.is_ghost { continue; }
                if sprite_slot_map.get(ai).and_then(|s| *s) != Some(slot) { continue; }
                if let Some(col) = sprite_col(a, ai) {
                    render_sprite(a, &col, false, &char_mask, &mut d);
                }
            }
        }
    }

    // 8. Ideal render / error overlay
    if opts.error_overlay || opts.ideal_render {
        let ideal = render_ideal(&vis_positions, glitch_color_active, glitch_frame);

        if opts.ideal_render {
            // Replace C64 output with ideal
            for y in 0..C64H {
                for x in 0..C64W {
                    let rgba_idx = (y * C64W + x) * 4;
                    let ideal_idx = (y * C64W + x) * 3;
                    d[rgba_idx] = ideal[ideal_idx];
                    d[rgba_idx + 1] = ideal[ideal_idx + 1];
                    d[rgba_idx + 2] = ideal[ideal_idx + 2];
                }
            }
        } else {
            // Error overlay: compare and tint differences red
            for y in 0..C64H {
                for x in 0..C64W {
                    let rgba_idx = (y * C64W + x) * 4;
                    let ideal_idx = (y * C64W + x) * 3;
                    let ar = d[rgba_idx];
                    let ag = d[rgba_idx + 1];
                    let ab = d[rgba_idx + 2];
                    let ir = ideal[ideal_idx];
                    let ig = ideal[ideal_idx + 1];
                    let ib = ideal[ideal_idx + 2];
                    if ar != ir || ag != ig || ab != ib {
                        d[rgba_idx] = 255;
                        d[rgba_idx + 1] = 0;
                        d[rgba_idx + 2] = 0;
                    }
                }
            }
        }
    }

    // 9. Mux capacity overlay — highlight scanlines at max sprites
    if opts.mux_overlay {
        for y in 0..C64H {
            let count = sl_counts.get(y).copied().unwrap_or(0) as usize;
            if count >= MAX_SPR_LINE {
                for x in 0..C64W {
                    let idx = (y * C64W + x) * 4;
                    d[idx] = ((d[idx] as u16 + 180) / 2) as u8;
                    d[idx + 1] = ((d[idx + 1] as u16 + 40) / 2) as u8;
                    d[idx + 2] = ((d[idx + 2] as u16 + 40) / 2) as u8;
                }
            }
        }
    }

    // 10. Compute memory stats (pixel-level occlusion) — uses pruned positions
    let mut mem_positions: Vec<&DiscPosition> = vis_positions
        .iter()
        .filter(|p| {
            let ox = p.x.round() as i32 - 8;
            let oy = p.y.round() as i32 - 8;
            ox + SPRITE_W as i32 > 0 && ox < C64W as i32 && oy + SPRITE_H as i32 > 0 && oy < C64H as i32
        })
        .collect();
    // Sort front-to-back (highest z first)
    mem_positions.sort_by(|a, b| b.z.total_cmp(&a.z));

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

    // 10. Compute stats (single pass)
    let (mut sprites, mut chars) = (0usize, 0usize);
    for a in &asgn {
        match a.mode {
            DiscMode::Sprite => sprites += 1,
            DiscMode::Char => chars += 1,
            DiscMode::Offscreen => {}
        }
    }

    // Compute pixel error only when needed (error overlay already computed ideal above)
    let pe = if opts.error_overlay || opts.ideal_render {
        // Ideal was already rendered in step 8 — reuse the comparison
        let c64_rgb = render_c64_image(&asgn, &sprite_slot_map, glitch_color_active, glitch_frame);
        let ideal = render_ideal(&vis_positions, glitch_color_active, glitch_frame);
        pixel_error(&c64_rgb, &ideal)
    } else {
        0
    };

    let stats = FrameStats {
        total: asgn.len(),
        visible: sprites + chars,
        sprites,
        chars,
        conflicts,
        max_sl,
        mux_overflows,
        mux_used,
        mem_discs,
        on_screen_count,
        mem_bytes,
        pixel_error: pe,
    };

    (d, stats, sl_counts)
}

// ============================================================
// Pixel helpers
// ============================================================

#[inline]
fn set_rgb(d: &mut [u8], idx: usize, col: [u8; 3]) {
    d[idx] = col[0];
    d[idx + 1] = col[1];
    d[idx + 2] = col[2];
}

// ============================================================
// Helper: render a single sprite into the pixel buffer
// ============================================================

fn render_sprite(
    a: &Assignment,
    col: &[u8; 3],
    bg_priority: bool,
    char_mask: &[u8],
    d: &mut [u8],
) {
    let spr_pixels = &*SPR_PIXELS;
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
            set_rgb(d, idx, *col);
        }
    }
}
