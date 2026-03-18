use std::collections::HashMap;
use std::sync::LazyLock;

use crate::data::*;

// ---------------------------------------------------------------------------
// Public structs
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct DiscPosition {
    pub id: usize,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub is_ghost: bool,
    pub ghost_depth: u8,
}

#[derive(Clone, Debug)]
pub struct StampCellPos {
    pub row: i32,
    pub col: i32,
    pub ch: u16,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DiscMode {
    Char,
    Sprite,
    Offscreen,
}

#[derive(Clone, Debug)]
pub struct Assignment {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub id: usize,
    pub is_ghost: bool,
    pub ghost_depth: u8,
    pub mode: DiscMode,
    pub stamp: Vec<StampCellPos>,
}

pub struct AllocResult {
    pub asgn: Vec<Assignment>,
    pub sl_counts: Vec<u8>,
    pub max_sl: u8,
    pub mux_overflows: u32,
    pub mux_used: u8,
    pub conflicts: u32,
    pub sprite_slot_map: Vec<Option<u8>>, // indexed by asgn index
}

pub struct FramePositions {
    pub positions: Vec<DiscPosition>,
    pub glitch_color_active: bool,
    pub glitch_frame: usize,
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// A char cell is considered "masked" by sprites if this many pixels are covered.
const MASK_THRESHOLD: u16 = 32; // 50% of 8x8 cell

// Offset by (-3, -2) to compensate for the circle bitmap center being at
// pixel (11, 10) within the 24x21 sprite, with render offset -8.
const CX: f64 = 157.0;
const CY: f64 = 98.0;
const GEO_R: f64 = 85.0;
const GEO_DIST: f64 = 400.0;
const F_END: f64 = 256.0;

// ---------------------------------------------------------------------------
// FONT definition
// ---------------------------------------------------------------------------

/// FONT: maps each letter to rows of column indices (matching the JS FONT object).
fn font_data() -> HashMap<char, Vec<Vec<i32>>> {
    let mut m = HashMap::new();
    m.insert(
        'E',
        vec![
            vec![0, 1, 2, 3, 4, 5, 6],
            vec![0],
            vec![0],
            vec![0, 1, 2, 3, 4],
            vec![0],
            vec![0],
            vec![0, 1, 2, 3, 4, 5, 6],
        ],
    );
    m.insert(
        'X',
        vec![
            vec![0, 6],
            vec![1, 5],
            vec![2, 4],
            vec![3],
            vec![2, 4],
            vec![1, 5],
            vec![0, 6],
        ],
    );
    m.insert(
        'T',
        vec![
            vec![0, 1, 2, 3, 4, 5, 6],
            vec![3],
            vec![3],
            vec![3],
            vec![3],
            vec![3],
            vec![3],
        ],
    );
    m.insert(
        'N',
        vec![
            vec![0, 6],
            vec![0, 1, 6],
            vec![0, 2, 6],
            vec![0, 3, 6],
            vec![0, 4, 6],
            vec![0, 5, 6],
            vec![0, 6],
        ],
    );
    m.insert(
        'D',
        vec![
            vec![0, 1, 2, 3, 4],
            vec![0, 5],
            vec![0, 6],
            vec![0, 6],
            vec![0, 6],
            vec![0, 5],
            vec![0, 1, 2, 3, 4],
        ],
    );
    m
}

const LOGO: &str = "EXTEND";
const CELL: f64 = 0.0595;

// ---------------------------------------------------------------------------
// Vertex / letter-range types used by the lazy statics
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Vert {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Debug)]
pub struct LetterRange {
    pub start: usize,
    pub end: usize,
}

/// Precomputed geometry: vertical layout vertices, horizontal layout vertices,
/// letter ranges, and total vertex count.
struct GeoData {
    verts_v: Vec<Vert>,
    verts_h: Vec<Vert>,
    letter_ranges: Vec<LetterRange>,
    n_verts: usize,
    e_vert_y: f64,
    d_vert_y: f64,
    e_vert_x: f64,
    d_vert_x: f64,
}

fn build_geo_data() -> GeoData {
    let font = font_data();

    // ------- Vertical layout (original FONT layout) -------
    // Letters stacked vertically with 3-row gaps between them.
    // Compute _mid_vy: total rows occupied = sum of each letter's row count + 3*(n-1)
    // plus padding (+6 in JS: `(ly - 3 + 6) / 2`)
    let mut ly: i32 = 0;
    for ch in LOGO.chars() {
        let rows = font[&ch].len() as i32;
        ly += rows + 3; // 3-row gap after every letter including last
    }
    // JS: _midVY = (ly - 3 + 6) / 2   (remove trailing gap then add 6)
    let mid_vy = (ly - 3 + 6) as f64 / 2.0;

    // Build vertical vertices and letter ranges
    let mut verts_v: Vec<Vert> = Vec::new();
    let mut letter_ranges: Vec<LetterRange> = Vec::new();
    let mut row_offset: i32 = 0;

    // For horizontal layout we also need a running x-offset per letter.
    // JS computes _totalW=47, _midHX=(47-1)/2=23
    let total_w: f64 = 47.0;
    let mid_hx: f64 = (total_w - 1.0) / 2.0;

    // Horizontal layout: letters laid out left-to-right.
    // Each letter occupies 7 columns (max column index 6) + 1 col per character width.
    // The JS code uses specific x-offsets for each letter; we reconstruct:
    // E starts at col 0, X at col 8, T at col 16, N at col 24, D at col 40 (with gaps).
    // Actually let's derive them the same way the JS does:
    // The JS positions each letter horizontally with the same FONT data but laid out in a row.
    // For horizontal layout, each letter's columns become its x positions,
    // and each letter's rows become its y positions, but all letters share the same y-center.
    //
    // Looking at the JS more carefully:
    //   Vertical layout: rows go down, columns go right, letter by letter vertically.
    //   Horizontal layout: letters placed side by side horizontally.
    //
    // For the vertical layout:
    //   For each letter, for each row r, for each column c in that row:
    //     vx = (c + 3 - _midHX) * CELL    -- but wait, this uses _midHX which is for horizontal.
    //
    // Actually, re-reading the JS more carefully, the vertex generation uses:
    //   GEO_VERTS_V[i] for the vertical-layout position
    //   GEO_VERTS_H[i] for the horizontal-layout position
    // Both arrays have the same length (N_VERTS) and are indexed identically
    // (same letter, same row within letter, same column within row).
    //
    // For the vertical layout:
    //   y = (row_offset + r + 3 - _midVY) * CELL    (3 = centering within letter cell)
    //   x = (c + 3 - _midHX) * CELL                 (centered horizontally... but for vertical
    //                                                  all letters share the same x-center)
    //
    // Hmm, looking at the JS constants:
    //   eVertY = (3 - _midVY) * CELL          -- "E"'s vertical center-y
    //   dVertY = (53 - _midVY) * CELL         -- "D"'s vertical center-y
    //   eVertX = (0 + 3 - _midHX) * CELL      -- "E"'s horizontal center-x
    //   dVertX = (40 + 3 - _midHX) * CELL     -- "D"'s horizontal center-x
    //
    // The 0 and 40 are the horizontal x-offsets for E and D respectively.
    // So horizontal x-offsets for EXTEND are: E=0, X=8, T=16, E=24, N=32, D=40
    // (each letter occupies 8 columns: 7 used + 1 gap)

    let h_offsets: Vec<i32> = vec![0, 8, 16, 24, 32, 40]; // one per letter in "EXTEND"

    let mut verts_h: Vec<Vert> = Vec::new();

    for (li, ch) in LOGO.chars().enumerate() {
        let letter_rows = &font[&ch];
        let start = verts_v.len();

        for (r, cols) in letter_rows.iter().enumerate() {
            for &c in cols {
                // Vertical layout vertex
                // For the vertical layout x: all letters are centered at x=0
                // but each dot's column offset is relative to the letter.
                // In the JS, vertical layout still uses the *horizontal* mid:
                //   Actually no. Let me re-derive from the JS eVertX / dVertX.
                //   eVertX = (0 + 3 - _midHX) * CELL   where 0 is E's h_offset
                //   So the vertical layout x for a dot at column c in letter li is:
                //     x = (h_offsets[li] + c - _midHX) * CELL   -- NO, that's the horizontal layout.
                //
                // OK let me think about this differently. The JS has two vertex arrays
                // that get morphed between. The vertical layout is the "logo stacked vertically"
                // and the horizontal layout is "logo in a row". Each vertex corresponds to
                // the same dot in the logo.
                //
                // Vertical layout (V):
                //   All letters centered at x=0 (or rather, each letter's columns centered),
                //   letters stacked top to bottom.
                //   vx = (c - 3) * CELL   (centering around column 3 of a 7-wide letter)
                //   vy = (row_offset + r + 3 - mid_vy) * CELL
                //
                // But the JS eVertX = (0 + 3 - _midHX) * CELL suggests the vertical layout
                // uses _midHX too. Let me check: eVertX is used as the camera pan target for
                // the E letter. In the horizontal layout, E is at h_offset=0, so its center
                // x = (0 + 3 - midHX) * CELL. That's the horizontal layout center.
                //
                // For the vertical layout, the pan target is panY=-eVertY*GEO_R*zoomFactor
                // (only Y panning). So the vertical layout has all letters at the same x.
                //
                // Let me reconsider the vertex definitions:
                // Vertical layout: letters stacked vertically, all centered at x=0.
                //   vx = (c - 3) * CELL    (column 3 is center of 7-wide glyph)
                //   vy = (row_offset + r - mid_vy + 3) * CELL
                //
                // Horizontal layout: letters in a row, all centered at y=0.
                //   hx = (h_offsets[li] + c - mid_hx) * CELL
                //   hy = (r - 3) * CELL    (row 3 is center of 7-tall glyph)
                //
                // Check: eVertY = (3 - mid_vy) * CELL  -- that's the y-center of the E letter
                // in the vertical layout. E's row_offset = 0, center row = 3,
                // so y_center = (0 + 3 - mid_vy) * CELL = (3 - mid_vy) * CELL. Correct!
                //
                // Check: dVertX = (40 + 3 - mid_hx) * CELL  -- that's the x-center of D
                // in the horizontal layout. D's h_offset = 40, center col = 3,
                // so x_center = (40 + 3 - mid_hx) * CELL. Correct!

                let vx = (c as f64 - 3.0) * CELL;
                let v_y = (row_offset as f64 + r as f64 + 3.0 - mid_vy) * CELL;
                verts_v.push(Vert { x: vx, y: v_y });

                // Horizontal layout vertex
                let hx = (h_offsets[li] as f64 + c as f64 - mid_hx) * CELL;
                let hy = (r as f64 - 3.0) * CELL;
                verts_h.push(Vert { x: hx, y: hy });
            }
        }

        let end = verts_v.len();
        letter_ranges.push(LetterRange { start, end });

        row_offset += letter_rows.len() as i32 + 3; // 3-row gap
    }

    let n_verts = verts_v.len();

    // Center on the middle row (r=3) of each letter, not the first row.
    // Vertex y = (row_offset + r + 3 - mid_vy) * CELL, so center = row_offset + 3 + 3.
    let e_vert_y = (0.0 + 3.0 + 3.0 - mid_vy) * CELL;   // E: row_offset=0
    let d_vert_y = (50.0 + 3.0 + 3.0 - mid_vy) * CELL;  // D: row_offset=50
    let e_vert_x = (0.0 + 3.0 - mid_hx) * CELL;
    let d_vert_x = (40.0 + 3.0 - mid_hx) * CELL;

    GeoData {
        verts_v,
        verts_h,
        letter_ranges,
        n_verts,
        e_vert_y,
        d_vert_y,
        e_vert_x,
        d_vert_x,
    }
}

static GEO: LazyLock<GeoData> = LazyLock::new(build_geo_data);

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

pub fn should_skip(z: f64) -> bool {
    z > FADE_CUTOFF
}

/// Smoothstep: t*t*(3 - 2*t)
#[inline]
fn smoothstep(t: f64) -> f64 {
    t * t * (3.0 - 2.0 * t)
}

/// Integer glitch PRNG matching the JS `glitchRand`.
#[inline]
fn glitch_rand(seed: i64) -> f64 {
    let v = (seed.wrapping_mul(1_103_515_245).wrapping_add(12345)) & 0x7fff_ffff;
    v as f64 / 0x7fff_ffff as f64
}

// ---------------------------------------------------------------------------
// get_stamp_cells  (public so allocate can use it too)
// ---------------------------------------------------------------------------

/// Returns the stamp cells and whether any cells were clipped by the viewport.
pub fn get_stamp_cells_with_clip(px: f64, py: f64) -> (Vec<StampCellPos>, bool) {
    let pxi = px as i32; // JS `px|0` — truncate toward zero
    let pyi = py as i32;
    let xs = ((pxi % 8) + 8) % 8;
    let ys = ((pyi % 8) + 8) % 8;
    let stamp_idx = (ys * 8 + xs) as usize;
    let all_stamps = &*STAMPS_TABLE;
    let stamp = &all_stamps[stamp_idx];

    let br_actual = (py / 8.0).floor() as i32 - 1;
    let bc_actual = (px / 8.0).floor() as i32 - 1;

    let mut cells = Vec::new();
    let mut clipped = false;
    for s in stamp {
        let row = br_actual + s.dr as i32;
        let col = bc_actual + s.dc as i32;
        if row >= 0 && row < ROWS as i32 && col >= 0 && col < COLS as i32 {
            cells.push(StampCellPos {
                row,
                col,
                ch: s.ch,
            });
        } else {
            clipped = true;
        }
    }
    (cells, clipped)
}

pub fn get_stamp_cells(px: f64, py: f64) -> Vec<StampCellPos> {
    get_stamp_cells_with_clip(px, py).0
}

// ---------------------------------------------------------------------------
// gen_positions
// ---------------------------------------------------------------------------

pub fn gen_positions(f: usize) -> FramePositions {
    let geo = &*GEO;

    let ff = f as f64;

    const TRAIL_LIFE: f64 = 12.0;
    const EXIT_FRAMES: f64 = 48.0;

    let e_range = &geo.letter_ranges[0];
    let morph_frames = ((P3_END - P2_END) as f64 * 0.2).floor() as usize;

    let morph_t = if f >= P2_END && f < P2_END + morph_frames {
        let mf = (f - P2_END) as f64;
        let t = mf / morph_frames as f64;
        smoothstep(t)
    } else if f >= P2_END + morph_frames {
        1.0
    } else {
        0.0
    };

    let e_vert_y = geo.e_vert_y;
    let d_vert_y = geo.d_vert_y;
    let e_vert_x = geo.e_vert_x;
    let d_vert_x = geo.d_vert_x;

    // Camera parameters
    let zoom_factor: f64;
    let pan_y: f64;
    let mut pan_x: f64 = 0.0;
    let stretch_x: f64 = 1.0;

    if f < P1_END {
        let t = ff / P1_END as f64;
        let e = smoothstep(t);
        zoom_factor = 2.5 + (1.8 - 2.5) * e;
        pan_y = -e_vert_y * GEO_R * zoom_factor;
    } else if f < P2_END {
        let t = (ff - P1_END as f64) / (P2_END - P1_END) as f64;
        let e = smoothstep(t);
        zoom_factor = 1.8 + (3.0 - 1.8) * e;
        let start_pan_y = -e_vert_y * GEO_R * 1.8;
        let end_pan_y = -d_vert_y * GEO_R * 3.0;
        pan_y = start_pan_y + (end_pan_y - start_pan_y) * e;
    } else {
        zoom_factor = 3.0;
        let cur_r15 = GEO_R * 3.0;
        if f < P3_END {
            let t = (ff - P2_END as f64) / (P3_END - P2_END) as f64;
            let settle_end = 0.2;
            if t < settle_end {
                let st = t / settle_end;
                let se = smoothstep(st);
                pan_y = (-d_vert_y * cur_r15) * (1.0 - se);
                pan_x = (-e_vert_x * cur_r15) * se;
            } else {
                let pt = (t - settle_end) / (1.0 - settle_end);
                pan_y = 0.0;
                let e_pan_x = -e_vert_x * cur_r15;
                let d_pan_x = -d_vert_x * cur_r15;
                pan_x = e_pan_x + (d_pan_x - e_pan_x) * pt;
            }
        } else {
            pan_y = 0.0;
            pan_x = -d_vert_x * cur_r15;
        }
    }

    // Rotation angles
    let p = if f < P2_END {
        0.0
    } else if f < P3_END {
        (ff - P2_END as f64) / F_END
    } else {
        (P3_END as f64 - P2_END as f64) / F_END
            + ((ff - P3_END as f64) / EXIT_FRAMES) * 0.5
    };

    let ax = (p * 2.0 * std::f64::consts::PI).sin() * 0.18;
    let ay = 0.0;
    let az = (p * 2.0 * std::f64::consts::PI).sin() * 0.04;

    // Beat pulse
    let bpm_period: f64 = 25.0;
    let beat_phase = (ff % bpm_period) / bpm_period;
    let pulse_amt = if beat_phase < 0.05 {
        beat_phase / 0.05
    } else if beat_phase < 0.5 {
        let decay = (beat_phase - 0.05) / 0.45;
        (-decay * 6.0).exp()
    } else {
        0.0
    };

    let pulse = 1.0 + 0.12 * pulse_amt;
    let cur_r = GEO_R * zoom_factor * pulse;

    let cx_rot = ax.cos();
    let sx_rot = ax.sin();
    let cy_rot = (ay as f64).cos();
    let sy_rot = (ay as f64).sin();
    let cz_rot = az.cos();
    let sz_rot = az.sin();

    let mut shake_x: f64 = 0.0;
    let mut shake_y: f64 = 0.0;
    if f >= P2_END {
        let shake_amt = pulse_amt * 6.0;
        shake_x = shake_amt * (ff * 7.3).sin();
        shake_y = shake_amt * (ff * 11.1).cos();
    }

    // Project closure
    let project = |vx: f64, vy: f64, vz: f64| -> (f64, f64, f64) {
        let mut x = vx;
        let mut y = vy;
        let mut z = vz;

        // Y-axis rotation
        let t1 = x * cy_rot + z * sy_rot;
        let t2 = -x * sy_rot + z * cy_rot;
        x = t1;
        z = t2;

        // X-axis rotation
        let t1 = y * cx_rot - z * sx_rot;
        let t2 = y * sx_rot + z * cx_rot;
        y = t1;
        z = t2;

        // Z-axis rotation
        let t1 = x * cz_rot - y * sz_rot;
        let t2 = x * sz_rot + y * cz_rot;
        x = t1;
        y = t2;

        x *= stretch_x;

        let s = GEO_DIST / (GEO_DIST + z * cur_r);
        let mut sx2 = pan_x * pulse + shake_x + x * cur_r * s;
        let mut sy2 = pan_y * pulse + shake_y + y * cur_r * s;

        // Barrel distortion
        let nx = sx2 / 160.0;
        let ny = sy2 / 100.0;
        let r2 = nx * nx + ny * ny;
        let k = 0.55;
        let distort = 1.0 + k * r2;
        sx2 *= distort;
        sy2 *= distort;

        (CX + sx2, CY + sy2, z)
    };

    // Disc birth timing
    let disc_birth = |i: usize| -> usize {
        if i < e_range.end {
            let local_idx = i - e_range.start;
            let local_count = e_range.end - e_range.start;
            return ((local_idx as f64 / local_count as f64) * P1_END as f64 * 0.7).floor()
                as usize;
        }
        for li in 1..geo.letter_ranges.len() {
            let lr = &geo.letter_ranges[li];
            if i >= lr.start && i < lr.end {
                let frames_per_letter =
                    ((P2_END - P1_END) as f64 / 5.0).floor() as usize;
                let local_idx = i - lr.start;
                let local_count = lr.end - lr.start;
                return P1_END
                    + (li - 1) * frames_per_letter
                    + ((local_idx as f64 / local_count as f64) * frames_per_letter as f64)
                        .floor() as usize;
            }
        }
        0
    };

    let is_exit = f >= P3_END && f < P4_END;
    let exit_t = if is_exit {
        (ff - P3_END as f64) / EXIT_FRAMES
    } else {
        0.0
    };
    let exit_life: f64 = 14.0;

    let exit_delay = |i: usize| -> f64 {
        for li in (0..geo.letter_ranges.len()).rev() {
            let lr = &geo.letter_ranges[li];
            if i >= lr.start && i < lr.end {
                let rev_li = geo.letter_ranges.len() - 1 - li;
                let local_idx = i - lr.start;
                let local_count = lr.end - lr.start;
                return rev_li as f64 * 0.1 + local_idx as f64 / local_count as f64 * 0.08;
            }
        }
        0.0
    };

    let exit_progress = |i: usize| -> f64 {
        if !is_exit {
            return 0.0;
        }
        let delay = exit_delay(i);
        ((exit_t - delay) / (exit_life / EXIT_FRAMES))
            .max(0.0)
            .min(1.0)
    };

    // E-letter scale (sized for the larger 21px circle)
    let e_scale: f64 = if f < P1_END {
        2.0
    } else if f < P2_END {
        let t = (ff - P1_END as f64) / (P2_END - P1_END) as f64;
        let e = smoothstep(t);
        2.0 + (1.0 - 2.0) * e
    } else {
        1.0
    };

    let mut result: Vec<DiscPosition> = Vec::with_capacity(geo.n_verts * 3);

    if f < P4_END {
        for i in 0..geo.n_verts {
            let birth = disc_birth(i);
            if !is_exit && f < birth {
                continue;
            }

            let v_v = &geo.verts_v[i];
            let v_h = &geo.verts_h[i];
            let vx = v_v.x + (v_h.x - v_v.x) * morph_t;
            let vy = v_v.y + (v_h.y - v_v.y) * morph_t;
            let vz = 0.0;

            let is_e_letter = i < e_range.end;

            let (vs_x, vs_y, vs_z) = if is_e_letter && e_scale != 1.0 {
                let ecx = 0.0;
                let ecy = e_vert_y + (0.0 - e_vert_y) * morph_t;
                (
                    ecx + (vx - ecx) * e_scale,
                    ecy + (vy - ecy) * e_scale,
                    vz,
                )
            } else {
                (vx, vy, vz)
            };

            let (fin_x, fin_y, fin_z) = project(vs_x, vs_y, vs_z);

            // Phase 1 & 2: trail fly-in
            if f < P2_END {
                let age = f as i64 - birth as i64;
                if age < 0 {
                    continue;
                }
                let fly_t = (age as f64 / TRAIL_LIFE).min(1.0);
                let ease_t = 1.0 - (1.0 - fly_t) * (1.0 - fly_t);

                let start_mul = if is_e_letter { 3.0 } else { 2.5 };
                let start_y_mul = if is_e_letter { 2.0 } else { 1.5 };
                let (start_x, start_y, start_z) =
                    project(vs_x * start_mul, vs_y * start_y_mul, vs_z);

                let cur_x = start_x + (fin_x - start_x) * ease_t;
                let cur_y = start_y + (fin_y - start_y) * ease_t;
                let cur_z = start_z + (fin_z - start_z) * ease_t;

                if fly_t >= 1.0 {
                    result.push(DiscPosition {
                        id: i,
                        x: fin_x,
                        y: fin_y,
                        z: fin_z,
                        is_ghost: false,
                        ghost_depth: 0,
                    });
                } else {
                    result.push(DiscPosition {
                        id: i,
                        x: cur_x,
                        y: cur_y,
                        z: cur_z,
                        is_ghost: false,
                        ghost_depth: 0,
                    });
                    for g in 1..=2u8 {
                        let gt = (ease_t - g as f64 * 0.12).max(0.0);
                        let gx = start_x + (fin_x - start_x) * gt;
                        let gy = start_y + (fin_y - start_y) * gt;
                        let gz = start_z + (fin_z - start_z) * gt;
                        result.push(DiscPosition {
                            id: i,
                            x: gx,
                            y: gy,
                            z: gz - g as f64 * 0.01,
                            is_ghost: true,
                            ghost_depth: g,
                        });
                    }
                }
                continue;
            }

            // Exit animation
            if is_exit {
                let ep = exit_progress(i);
                if ep >= 1.0 {
                    continue;
                }
                let ease_out = ep * ep * ep;
                let dx = fin_x - CX;
                let dy = fin_y - CY;
                let dist = (dx * dx + dy * dy).sqrt().max(1.0);
                let spin_angle = ease_out * 1.5;
                let cs = spin_angle.cos();
                let sn = spin_angle.sin();
                let rdx = dx * cs - dy * sn;
                let rdy = dx * sn + dy * cs;
                let fly_dist = 400.0 * ease_out;
                let cur_x = fin_x + rdx / dist * fly_dist;
                let cur_y = fin_y + rdy / dist * fly_dist;

                if cur_x < -60.0 || cur_x > C64W as f64 + 60.0 || cur_y < -60.0 || cur_y > C64H as f64 + 60.0
                {
                    continue;
                }

                result.push(DiscPosition {
                    id: i,
                    x: cur_x,
                    y: cur_y,
                    z: fin_z,
                    is_ghost: false,
                    ghost_depth: 0,
                });

                for g in 1..=2u8 {
                    let gep = (ease_out - g as f64 * 0.06).max(0.0);
                    let gfd = 400.0 * gep;
                    let gsa = gep * 1.5;
                    let gcs = gsa.cos();
                    let gsn = gsa.sin();
                    let gdx = dx * gcs - dy * gsn;
                    let gdy = dx * gsn + dy * gcs;
                    result.push(DiscPosition {
                        id: i,
                        x: fin_x + gdx / dist * gfd,
                        y: fin_y + gdy / dist * gfd,
                        z: fin_z - g as f64 * 0.01,
                        is_ghost: true,
                        ghost_depth: g,
                    });
                }
                continue;
            }

            // Normal on-screen check
            let margin = 40.0;
            let on_screen = fin_x > -margin
                && fin_x < C64W as f64 + margin
                && fin_y > -margin
                && fin_y < C64H as f64 + margin;
            if !on_screen {
                continue;
            }
            result.push(DiscPosition {
                id: i,
                x: fin_x,
                y: fin_y,
                z: fin_z,
                is_ghost: false,
                ghost_depth: 0,
            });
        }
    }

    let glitch_color_active = glitch_rand(ff as i64 * 17 + 7) > 0.92;

    // Sort by z (front-to-back)
    result.sort_by(|a, b| a.z.total_cmp(&b.z));

    FramePositions {
        positions: result,
        glitch_color_active,
        glitch_frame: f,
    }
}

// ---------------------------------------------------------------------------
// allocate
// ---------------------------------------------------------------------------

fn try_mux_fit(sprite_list: &[(f64, f64)]) -> bool {
    // Each entry is (x, y). We need topY = floor(y) - 8.
    let mut infos: Vec<(usize, i32)> = sprite_list
        .iter()
        .enumerate()
        .map(|(i, &(_, y))| (i, (y.floor() as i32) - 8))
        .collect();
    infos.sort_by_key(|&(_, top_y)| top_y);

    let mut slots = [-999i32; 8];
    for &(_, top_y) in &infos {
        let mut ok = false;
        for s in 0..8 {
            if slots[s] <= top_y {
                slots[s] = top_y + MUX_H as i32;
                ok = true;
                break;
            }
        }
        if !ok {
            return false;
        }
    }
    true
}

/// Build a cell-level sprite coverage map: for each char cell, count how many
/// sprite pixels from foreground sprites (z <= 0) cover it.
fn build_sprite_coverage(
    vis: &[DiscPosition],
    mode: &[DiscMode],
    spr_pixels: &[u8],
) -> Vec<u16> {
    let mut coverage = vec![0u16; ROWS * COLS];
    for (i, p) in vis.iter().enumerate() {
        if mode[i] != DiscMode::Sprite || p.z > 0.0 {
            continue;
        }
        add_sprite_to_coverage(&mut coverage, p, spr_pixels);
    }
    coverage
}

/// Incrementally add one foreground sprite's pixel footprint to the coverage map.
fn add_sprite_to_coverage(
    coverage: &mut [u16],
    p: &DiscPosition,
    spr_pixels: &[u8],
) {
    if p.z > 0.0 {
        return;
    }
    let ox = p.x.floor() as i32 - 8;
    let oy = p.y.floor() as i32 - 8;
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
            let cell_r = sy as usize / CHH;
            let cell_c = sx as usize / CHW;
            if cell_r < ROWS && cell_c < COLS {
                coverage[cell_r * COLS + cell_c] += 1;
            }
        }
    }
}

/// Check if a cell is masked by sprite coverage.
#[inline]
fn is_cell_masked(coverage: &[u16], row: i32, col: i32) -> bool {
    let idx = row as usize * COLS + col as usize;
    coverage[idx] >= MASK_THRESHOLD
}

pub fn allocate(positions: &[DiscPosition]) -> AllocResult {
    if positions.is_empty() {
        return AllocResult {
            asgn: Vec::new(),
            sl_counts: vec![0u8; C64H],
            max_sl: 0,
            mux_overflows: 0,
            mux_used: 0,
            conflicts: 0,
            sprite_slot_map: Vec::new(),
        };
    }

    // Determine visible positions and their stamps
    let mut vis: Vec<DiscPosition> = Vec::new();
    let mut vis_idx: Vec<usize> = Vec::new();
    let mut force_sprite: Vec<bool> = Vec::new();
    let mut stamp_data: Vec<Vec<StampCellPos>> = Vec::new();

    for (i, p) in positions.iter().enumerate() {
        if p.x >= -12.0 && p.x < C64W as f64 + 12.0 && p.y >= -12.0 && p.y < C64H as f64 + 12.0 {
            let (cells, clipped) = get_stamp_cells_with_clip(p.x, p.y);
            let near_edge = p.y < 16.0 || p.y > C64H as f64 - 16.0 || p.x < 16.0 || p.x > C64W as f64 - 16.0;
            if !cells.is_empty() {
                vis.push(p.clone());
                vis_idx.push(i);
                // Force sprite if stamp was clipped by viewport or disc is near edge with no stamp
                force_sprite.push(clipped);
                stamp_data.push(cells);
            } else if near_edge {
                vis.push(p.clone());
                vis_idx.push(i);
                force_sprite.push(true);
                stamp_data.push(cells); // empty vec
            }
        }
    }

    let vis_len = vis.len();
    let mut mode: Vec<DiscMode> = vec![DiscMode::Char; vis_len];
    let mut sprite_vis: Vec<usize> = Vec::new();

    // Force-sprite pass — clipped stamps must be sprite or offscreen, never char
    for i in 0..vis_len {
        if force_sprite[i] {
            let mut test_list: Vec<(f64, f64)> =
                sprite_vis.iter().map(|&vi| (vis[vi].x, vis[vi].y)).collect();
            test_list.push((vis[i].x, vis[i].y));
            if try_mux_fit(&test_list) {
                mode[i] = DiscMode::Sprite;
                sprite_vis.push(i);
            } else {
                // Can't fit in mux — don't render a clipped char stamp
                mode[i] = DiscMode::Offscreen;
            }
        }
    }

    // Build conflicts helper — only counts conflicts in cells NOT masked by sprites
    let build_conflicts = |mode: &[DiscMode], stamp_data: &[Vec<StampCellPos>], coverage: &[u16]| -> Vec<u16> {
        let mut cell_owners: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
        for i in 0..vis_len {
            if mode[i] != DiscMode::Char {
                continue;
            }
            for s in &stamp_data[i] {
                cell_owners.entry((s.row, s.col)).or_default().push(i);
            }
        }
        let mut cc = vec![0u16; vis_len];
        for (&(row, col), owners) in &cell_owners {
            if owners.len() > 1 && !is_cell_masked(coverage, row, col) {
                for &o in owners {
                    cc[o] += (owners.len() - 1) as u16;
                }
            }
        }
        cc
    };

    // Build initial sprite coverage from force-sprite assignments
    let spr_pixels = &*SPR_PIXELS;
    let mut coverage = build_sprite_coverage(&vis, &mode, spr_pixels);

    // Iterative promotion to sprite
    for _iter in 0..vis_len {
        let cc = build_conflicts(&mode, &stamp_data, &coverage);
        let mut cands: Vec<(usize, u16)> = Vec::new();
        for i in 0..vis_len {
            if mode[i] == DiscMode::Char && cc[i] > 0 {
                cands.push((i, cc[i]));
            }
        }
        if cands.is_empty() {
            break;
        }
        cands.sort_by(|a, b| b.1.cmp(&a.1));

        let mut promoted = false;
        for &(cand_i, _) in &cands {
            let mut test_list: Vec<(f64, f64)> =
                sprite_vis.iter().map(|&vi| (vis[vi].x, vis[vi].y)).collect();
            test_list.push((vis[cand_i].x, vis[cand_i].y));
            if try_mux_fit(&test_list) {
                mode[cand_i] = DiscMode::Sprite;
                sprite_vis.push(cand_i);
                // Incrementally update coverage if this is a foreground sprite
                add_sprite_to_coverage(&mut coverage, &vis[cand_i], spr_pixels);
                promoted = true;
                break;
            }
        }
        if !promoted {
            break;
        }
    }

    // Nudge pass — coverage is stable here (no new sprite promotions)
    {
        let offsets: [(i32, i32); 12] = [
            (-1, 0),
            (1, 0),
            (0, -1),
            (0, 1),
            (-1, -1),
            (1, -1),
            (-1, 1),
            (1, 1),
            (-2, 0),
            (2, 0),
            (0, -2),
            (0, 2),
        ];

        for _nr in 0..3 {
            let mut improved = false;

            // Build cell owner map for char-mode discs
            let mut co: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
            for i in 0..vis_len {
                if mode[i] != DiscMode::Char {
                    continue;
                }
                for s in &stamp_data[i] {
                    co.entry((s.row, s.col)).or_default().push(i);
                }
            }

            // Find all discs involved in unmasked conflicts
            let mut cds: Vec<usize> = Vec::new();
            for (&(row, col), owners) in &co {
                if owners.len() > 1 && !is_cell_masked(&coverage, row, col) {
                    for &x in owners {
                        cds.push(x);
                    }
                }
            }
            cds.sort_unstable();
            cds.dedup();

            for &di in &cds {
                if mode[di] != DiscMode::Char {
                    continue;
                }
                // Count current unmasked conflicts for this disc
                let mut cur = 0i32;
                for s in &stamp_data[di] {
                    if is_cell_masked(&coverage, s.row, s.col) {
                        continue;
                    }
                    if let Some(o) = co.get(&(s.row, s.col)) {
                        if o.len() > 1 {
                            cur += 1;
                        }
                    }
                }
                if cur == 0 {
                    continue;
                }

                let mut best_off: Option<(i32, i32)> = None;
                let mut best_c = cur;
                let mut best_s: Option<Vec<StampCellPos>> = None;
                let ox = vis[di].x;
                let oy = vis[di].y;

                for &(dx, dy) in &offsets {
                    let ns = get_stamp_cells(ox + dx as f64, oy + dy as f64);
                    if ns.is_empty() {
                        continue;
                    }
                    let mut nc = 0i32;
                    for s in &ns {
                        if is_cell_masked(&coverage, s.row, s.col) {
                            continue;
                        }
                        if let Some(o) = co.get(&(s.row, s.col)) {
                            let oc = o.iter().filter(|&&x| x != di).count();
                            if oc > 0 {
                                nc += 1;
                            }
                        }
                    }
                    if nc < best_c {
                        best_c = nc;
                        best_off = Some((dx, dy));
                        best_s = Some(ns);
                    }
                }

                if let (Some(off), Some(new_stamp)) = (best_off, best_s) {
                    if best_c < cur {
                        // Remove old stamp entries from co
                        for s in &stamp_data[di] {
                            if let Some(o) = co.get_mut(&(s.row, s.col)) {
                                if let Some(idx) = o.iter().position(|&x| x == di) {
                                    o.remove(idx);
                                }
                            }
                        }
                        // Update position and stamp
                        vis[di].x = ox + off.0 as f64;
                        vis[di].y = oy + off.1 as f64;
                        stamp_data[di] = new_stamp.clone();
                        // Add new stamp entries to co
                        for s in &new_stamp {
                            co.entry((s.row, s.col)).or_default().push(di);
                        }
                        improved = true;
                    }
                }
            }

            if !improved {
                break;
            }
        }
    }

    // Build final assignment array (one per original position)
    let mut asgn: Vec<Assignment> = positions
        .iter()
        .map(|p| Assignment {
            x: p.x,
            y: p.y,
            z: p.z,
            id: p.id,
            is_ghost: p.is_ghost,
            ghost_depth: p.ghost_depth,
            mode: DiscMode::Offscreen,
            stamp: Vec::new(),
        })
        .collect();

    for vi in 0..vis_len {
        let gi = vis_idx[vi];
        asgn[gi].mode = mode[vi];
        asgn[gi].stamp = stamp_data[vi].clone();
        asgn[gi].x = vis[vi].x;
        asgn[gi].y = vis[vi].y;
    }

    // Rebuild final coverage from all sprite assignments for accurate conflict count
    let final_coverage = {
        let mut fc = vec![0u16; ROWS * COLS];
        for a in &asgn {
            if a.mode == DiscMode::Sprite && a.z <= 0.0 {
                let ox = a.x.floor() as i32 - 8;
                let oy = a.y.floor() as i32 - 8;
                for sr in 0..SPRITE_H {
                    let sy = oy + sr as i32;
                    if sy < 0 || sy >= C64H as i32 { continue; }
                    for sc in 0..SPRITE_W {
                        if spr_pixels[sr * SPRITE_W + sc] == 0 { continue; }
                        let sx = ox + sc as i32;
                        if sx < 0 || sx >= C64W as i32 { continue; }
                        let cell_r = sy as usize / CHH;
                        let cell_c = sx as usize / CHW;
                        if cell_r < ROWS && cell_c < COLS {
                            fc[cell_r * COLS + cell_c] += 1;
                        }
                    }
                }
            }
        }
        fc
    };

    // Count visible char conflicts (not masked by foreground sprites)
    let mut char_conflicts: u32 = 0;
    {
        let mut cm: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
        for (i, a) in asgn.iter().enumerate() {
            if a.mode != DiscMode::Char {
                continue;
            }
            for s in &a.stamp {
                cm.entry((s.row, s.col)).or_default().push(i);
            }
        }
        for (&(row, col), owners) in &cm {
            if owners.len() > 1 && !is_cell_masked(&final_coverage, row, col) {
                char_conflicts += 1;
            }
        }
    }

    // Assign sprite mux slots
    struct SpriteInfo {
        idx: usize,
        top_y: i32,
    }

    let mut sprite_infos: Vec<SpriteInfo> = Vec::new();
    for (ai, a) in asgn.iter().enumerate() {
        if a.mode != DiscMode::Sprite {
            continue;
        }
        let top_y = (a.y.floor() as i32) - 8;
        sprite_infos.push(SpriteInfo {
            idx: ai,
            top_y,
        });
    }
    sprite_infos.sort_by_key(|si| si.top_y);

    let mut slot_free_at = [-999i32; 8];
    let mut mux_used: u8 = 0;
    let mut sprite_slot_lookup: HashMap<usize, u8> = HashMap::new();

    let mut mux_overflows: u32 = 0;
    for si in &sprite_infos {
        let mut assigned = false;
        for s in 0..8u8 {
            if slot_free_at[s as usize] <= si.top_y {
                slot_free_at[s as usize] = si.top_y + MUX_H as i32;
                sprite_slot_lookup.insert(si.idx, s);
                mux_used = mux_used.max(s + 1);
                assigned = true;
                break;
            }
        }
        if !assigned {
            mux_overflows += 1;
        }
    }

    // Build sl_counts
    let mut sl_counts = vec![0u8; C64H];
    for si in &sprite_infos {
        if sprite_slot_lookup.get(&si.idx).is_none() {
            continue;
        }
        let top = si.top_y.max(0) as usize;
        let bot = (si.top_y + SPRITE_H as i32 - 1).min(C64H as i32 - 1) as usize;
        for sl in top..=bot {
            sl_counts[sl] += 1;
        }
    }

    let max_sl = *sl_counts.iter().max().unwrap_or(&0);

    // Build sprite_slot_map indexed by asgn index
    let mut sprite_slot_map: Vec<Option<u8>> = vec![None; asgn.len()];
    for (&idx, &slot) in &sprite_slot_lookup {
        sprite_slot_map[idx] = Some(slot);
    }

    AllocResult {
        asgn,
        sl_counts,
        max_sl,
        mux_overflows,
        mux_used,
        conflicts: char_conflicts,
        sprite_slot_map,
    }
}
