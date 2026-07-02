use crate::data::*;
use crate::SeqFrame;
use crate::render::shade_index;
use crate::sim::*;

// ---------------------------------------------------------------------------
// C64 color mapping
// ---------------------------------------------------------------------------

/// Maps shade ramp index (0..6) to C64 color index.
/// Ordered dark-to-light matching the shade ramp.
const C64_SHADE_COLORS: [u8; 7] = [
    0,   // 0: darkest  -> black
    6,   // 1: DKBLUE   -> blue
    6,   // 2: BLUE     -> blue
    4,   // 3: PURPLE   -> purple
    14,  // 4: LTBLUE   -> light blue
    15,  // 5: CYAN     -> light grey
    1,   // 6: WHITE    -> white
];

const C64_COLOR_NAMES: [&str; 16] = [
    "black", "white", "red", "cyan", "purple", "green", "blue", "yellow",
    "orange", "brown", "light red", "dark grey", "grey", "light green",
    "light blue", "light grey",
];

// ---------------------------------------------------------------------------
// Export data structures
// ---------------------------------------------------------------------------

pub(crate) struct ExportStamp {
    screen_offset: u16,  // row * 40 + col (0-999)
    stamp_index: u8,     // 0-63
}

pub(crate) struct ExportStream {
    c64_color: u8,
    stamps: Vec<ExportStamp>,
}

pub(crate) struct ExportFrame {
    streams: Vec<ExportStream>,
}

// ---------------------------------------------------------------------------
// Build export data from all frames
// ---------------------------------------------------------------------------

pub fn build_export_data(prune_dist: f64, seq_frames: &[SeqFrame]) -> Vec<ExportFrame> {
    let mut frames = Vec::with_capacity(TOTAL_FRAMES);

    for f in 0..TOTAL_FRAMES {
        let fp = gen_positions(f);
        let mut vis: Vec<DiscPosition> = fp.positions.iter()
            .filter(|p| !should_skip(p.z))
            .cloned()
            .collect();
        prune_by_proximity(&mut vis, prune_dist);

        // Use optimized allocation if available, else default allocator
        let (asgn, _sprite_slot_map) = if let Some(ref state) = seq_frames[f].result {
            (state.asgn.clone(), state.sprite_slot_map.clone())
        } else {
            let alloc = allocate(&vis);
            (alloc.asgn, alloc.sprite_slot_map)
        };

        let glitch_color = fp.glitch_color_active;
        let glitch_frame = fp.glitch_frame;

        // Collect char stamps with shade index and effective z
        struct StampEntry {
            shade_idx: usize,
            eff_z: f64,
            screen_offset: u16,
            stamp_index: u8,
        }

        let mut entries: Vec<StampEntry> = Vec::new();

        for a in &asgn {
            if a.mode != DiscMode::Char { continue; }

            let si = shade_index(a, glitch_color, glitch_frame);
            let eff_z = if a.is_ghost { a.z + 10.0 + a.ghost_depth as f64 } else { a.z };

            // Compute stamp index from sub-pixel position
            let pxi = a.x as i32;
            let pyi = a.y as i32;
            let xs = ((pxi % 8) + 8) % 8;
            let ys = ((pyi % 8) + 8) % 8;
            let stamp_idx = (ys * 8 + xs) as u8;

            // Compute base cell
            let br = (a.y / 8.0).floor() as i32 - 1;
            let bc = (a.x / 8.0).floor() as i32 - 1;
            if br < 0 || bc < 0 || br >= ROWS as i32 || bc >= COLS as i32 {
                continue; // skip edge stamps that shouldn't be char mode
            }

            let screen_offset = (br as u16) * (COLS as u16) + (bc as u16);

            entries.push(StampEntry {
                shade_idx: si,
                eff_z,
                screen_offset,
                stamp_index: stamp_idx,
            });
        }

        // Group by shade index, order groups dark-to-light (ascending)
        // Within each group, order by effective z descending (back first, front writes last)
        entries.sort_by(|a, b| {
            a.shade_idx.cmp(&b.shade_idx)
                .then(b.eff_z.total_cmp(&a.eff_z)) // back first within group
        });

        // Build streams grouped by C64 color
        let mut streams: Vec<ExportStream> = Vec::new();
        let mut i = 0;
        while i < entries.len() {
            let c64_col = C64_SHADE_COLORS[entries[i].shade_idx];
            let mut stamps = Vec::new();
            while i < entries.len() && C64_SHADE_COLORS[entries[i].shade_idx] == c64_col {
                stamps.push(ExportStamp {
                    screen_offset: entries[i].screen_offset,
                    stamp_index: entries[i].stamp_index,
                });
                i += 1;
            }
            streams.push(ExportStream { c64_color: c64_col, stamps });
        }

        frames.push(ExportFrame { streams });
    }

    frames
}

// ---------------------------------------------------------------------------
// Binary serialization
// ---------------------------------------------------------------------------

pub fn serialize_binary(frames: &[ExportFrame]) -> Vec<u8> {
    let num_frames = frames.len() as u16;
    let header_size = 2 + (num_frames as usize) * 2; // num_frames + offset table

    let mut data = Vec::new();

    // Write frame data first to compute offsets
    let mut frame_data: Vec<Vec<u8>> = Vec::new();
    for frame in frames {
        let mut fd = Vec::new();
        for (si, stream) in frame.streams.iter().enumerate() {
            let is_last = si == frame.streams.len() - 1;
            let count = stream.stamps.len().min(127) as u8;
            let num_byte = if is_last { count | 0x80 } else { count };
            fd.push(num_byte);
            fd.push(stream.c64_color);
            for stamp in &stream.stamps[..count as usize] {
                let word = (stamp.screen_offset & 0x3FF)
                    | ((stamp.stamp_index as u16 & 0x3F) << 10);
                fd.push((word & 0xFF) as u8);
                fd.push((word >> 8) as u8);
            }
        }
        // Empty frame: emit a single empty last-stream
        if frame.streams.is_empty() {
            fd.push(0x80); // count=0, last=true
            fd.push(0);    // color=black
        }
        frame_data.push(fd);
    }

    // Compute offsets — identical frames share their data (dedup)
    let mut seen: std::collections::HashMap<&[u8], u16> = std::collections::HashMap::new();
    let mut offset = header_size;
    let mut offsets: Vec<u16> = Vec::new();
    let mut unique: Vec<&[u8]> = Vec::new();
    for fd in &frame_data {
        if let Some(&off) = seen.get(fd.as_slice()) {
            offsets.push(off);
        } else {
            seen.insert(fd.as_slice(), offset as u16);
            offsets.push(offset as u16);
            unique.push(fd.as_slice());
            offset += fd.len();
        }
    }

    // Write header
    data.push((num_frames & 0xFF) as u8);
    data.push((num_frames >> 8) as u8);
    for off in &offsets {
        data.push((off & 0xFF) as u8);
        data.push((off >> 8) as u8);
    }

    // Write frame data
    for fd in &unique {
        data.extend_from_slice(fd);
    }

    data
}

// ---------------------------------------------------------------------------
// C-style text serialization
// ---------------------------------------------------------------------------

pub fn serialize_text(frames: &[ExportFrame]) -> String {
    let bin = serialize_binary(frames);
    let num_frames = frames.len();

    let mut s = String::new();
    s.push_str(&format!("// C64 Stamp Export — {} frames, {} bytes\n", num_frames, bin.len()));
    s.push_str(&format!("// Format: streams ordered dark-to-light, stamps back-to-front\n\n"));

    // Header
    s.push_str(&format!("// Header: num_frames={}\n", num_frames));
    s.push_str("const unsigned char stamp_data[] = {\n");

    let mut pos = 2;

    // num_frames
    s.push_str(&format!("  0x{:02x}, 0x{:02x},  // num_frames={}\n",
        bin[0], bin[1], num_frames));

    // Frame offset table
    s.push_str("  // Frame offset table\n");
    let mut frame_offsets: Vec<u16> = Vec::with_capacity(num_frames);
    for f in 0..num_frames {
        let off = bin[pos] as u16 | ((bin[pos + 1] as u16) << 8);
        frame_offsets.push(off);
        s.push_str(&format!("  0x{:02x}, 0x{:02x},  // frame {} -> offset {}\n",
            bin[pos], bin[pos + 1], f, off));
        pos += 2;
    }

    // Frame data — deduplicated frames reference an earlier frame's data
    let mut first_frame_at: std::collections::HashMap<u16, usize> = std::collections::HashMap::new();
    for (f, frame) in frames.iter().enumerate() {
        let off = frame_offsets[f];
        if let Some(&orig) = first_frame_at.get(&off) {
            s.push_str(&format!("\n  // --- Frame {} : duplicate of frame {} (offset {}) ---\n",
                f, orig, off));
            continue;
        }
        first_frame_at.insert(off, f);

        let total_stamps: usize = frame.streams.iter().map(|s| s.stamps.len()).sum();
        s.push_str(&format!("\n  // --- Frame {} : {} streams, {} stamps ---\n",
            f, frame.streams.len(), total_stamps));

        for (si, stream) in frame.streams.iter().enumerate() {
            let is_last = si == frame.streams.len() - 1;
            let count = stream.stamps.len().min(127);
            let color_name = if (stream.c64_color as usize) < C64_COLOR_NAMES.len() {
                C64_COLOR_NAMES[stream.c64_color as usize]
            } else { "?" };

            s.push_str(&format!("  0x{:02x}, 0x{:02x},  // stream: {} stamps, color={} ({}){}\n",
                bin[pos], bin[pos + 1],
                count, stream.c64_color, color_name,
                if is_last { " [LAST]" } else { "" }));
            pos += 2;

            for _stamp in &stream.stamps[..count] {
                let word = bin[pos] as u16 | ((bin[pos + 1] as u16) << 8);
                let scr_off = word & 0x3FF;
                let stmp_idx = (word >> 10) & 0x3F;
                let row = scr_off / 40;
                let col = scr_off % 40;
                s.push_str(&format!("  0x{:02x}, 0x{:02x},  // offset={} (r{},c{}), stamp={}\n",
                    bin[pos], bin[pos + 1], scr_off, row, col, stmp_idx));
                pos += 2;
            }
        }

        // Handle empty frame marker
        if frame.streams.is_empty() {
            s.push_str(&format!("  0x{:02x}, 0x{:02x},  // empty frame\n", bin[pos], bin[pos + 1]));
            pos += 2;
        }
    }

    s.push_str("};\n");
    s
}

// ===========================================================================
// Sprite export (v3 flat format — no color/priority)
// ===========================================================================

pub(crate) struct ExportSpriteFrame {
    sprites: Vec<(u16, u8)>, // (x, y) VIC coords, Y-sorted ascending
}

pub fn build_sprite_export_data(prune_dist: f64, seq_frames: &[SeqFrame]) -> Vec<ExportSpriteFrame> {
    let mut frames = Vec::with_capacity(TOTAL_FRAMES);

    for f in 0..TOTAL_FRAMES {
        let fp = gen_positions(f);
        let mut vis: Vec<DiscPosition> = fp.positions.iter()
            .filter(|p| !should_skip(p.z))
            .cloned()
            .collect();
        prune_by_proximity(&mut vis, prune_dist);

        let (asgn, sprite_slot_map) = if let Some(ref state) = seq_frames[f].result {
            (state.asgn.clone(), state.sprite_slot_map.clone())
        } else {
            let alloc = allocate(&vis);
            (alloc.asgn, alloc.sprite_slot_map)
        };

        let mut sprites: Vec<(u16, u8)> = Vec::new();

        for (ai, a) in asgn.iter().enumerate() {
            if a.mode != DiscMode::Sprite { continue; }
            if a.is_ghost { continue; }
            if sprite_slot_map.get(ai).and_then(|s| *s).is_none() { continue; }

            let ox = a.x.floor() as i32 - 8;
            let oy = a.y.floor() as i32 - 8;

            let x = (ox + 24).max(0) as u16;
            let y = (oy + 51).clamp(0, 255) as u8;

            sprites.push((x, y));
        }

        sprites.sort_by_key(|&(_, y)| y);

        frames.push(ExportSpriteFrame { sprites });
    }

    frames
}

// ---------------------------------------------------------------------------
// Sprite binary serialization (v3 flat format)
// ---------------------------------------------------------------------------
//
// Header:
//   [num_frames: u16]
//   [frame_offsets: u16 × num_frames]  (0xFFFF = empty frame)
//
// Per frame:
//   [total_count: u8]
//   [x_hi_init: u8]              bits for sprites 0-7
//   [x_hi_overflow: u8 × N]     ceil((count-8)/8) bytes, 0 if count <= 8
//   [y, x_lo] × total_count     2 bytes per sprite, Y-sorted ascending
// ---------------------------------------------------------------------------

pub fn serialize_sprite_binary(frames: &[ExportSpriteFrame]) -> Vec<u8> {
    let num_frames = frames.len() as u16;
    let header_size = 2 + (num_frames as usize) * 2;

    let mut frame_data: Vec<Option<Vec<u8>>> = Vec::new();
    for frame in frames {
        if frame.sprites.is_empty() {
            frame_data.push(None);
        } else {
            let mut fd = Vec::new();
            let count = frame.sprites.len();
            fd.push(count as u8);

            // x_hi bytes: first byte covers sprites 0-7, then one byte per 8 additional
            let x_hi_bytes = 1 + if count > 8 { (count - 8 + 7) / 8 } else { 0 };
            let mut x_hi = vec![0u8; x_hi_bytes];
            for (i, &(x, _)) in frame.sprites.iter().enumerate() {
                if x > 255 {
                    x_hi[i / 8] |= 1 << (i % 8);
                }
            }
            fd.extend_from_slice(&x_hi);

            // Sprite data: [y, x_lo] per sprite
            for &(x, y) in &frame.sprites {
                fd.push(y);
                fd.push((x & 0xFF) as u8);
            }
            frame_data.push(Some(fd));
        }
    }

    // Compute offsets — identical frames share their data (dedup)
    let mut seen: std::collections::HashMap<&[u8], u16> = std::collections::HashMap::new();
    let mut offset = header_size;
    let mut offsets: Vec<u16> = Vec::new();
    let mut unique: Vec<&[u8]> = Vec::new();
    for fd in &frame_data {
        match fd {
            None => offsets.push(0xFFFF),
            Some(bytes) => {
                if let Some(&off) = seen.get(bytes.as_slice()) {
                    offsets.push(off);
                } else {
                    seen.insert(bytes.as_slice(), offset as u16);
                    offsets.push(offset as u16);
                    unique.push(bytes.as_slice());
                    offset += bytes.len();
                }
            }
        }
    }

    let mut data = Vec::new();
    data.push((num_frames & 0xFF) as u8);
    data.push((num_frames >> 8) as u8);
    for off in &offsets {
        data.push((off & 0xFF) as u8);
        data.push((off >> 8) as u8);
    }
    for fd in &unique {
        data.extend_from_slice(fd);
    }

    data
}

// ---------------------------------------------------------------------------
// Sprite text serialization (v3 flat format)
// ---------------------------------------------------------------------------

pub fn serialize_sprite_text(frames: &[ExportSpriteFrame]) -> String {
    let bin = serialize_sprite_binary(frames);
    let num_frames = frames.len();

    let mut s = String::new();
    s.push_str(&format!("// C64 Sprite Export v3 — {} frames, {} bytes\n", num_frames, bin.len()));
    s.push_str("// Per frame: [count] [x_hi_init] [x_hi_overflow...] [y, x_lo]...\n\n");

    s.push_str("const unsigned char sprite_data[] = {\n");

    let mut pos = 0;

    s.push_str(&format!("  0x{:02x}, 0x{:02x},  // num_frames={}\n",
        bin[0], bin[1], num_frames));
    pos += 2;

    s.push_str("  // Frame offset table\n");
    let mut frame_offsets: Vec<u16> = Vec::with_capacity(num_frames);
    for f in 0..num_frames {
        let off = bin[pos] as u16 | ((bin[pos + 1] as u16) << 8);
        frame_offsets.push(off);
        if off == 0xFFFF {
            s.push_str(&format!("  0xff, 0xff,  // frame {} -> empty\n", f));
        } else {
            s.push_str(&format!("  0x{:02x}, 0x{:02x},  // frame {} -> offset {}\n",
                bin[pos], bin[pos + 1], f, off));
        }
        pos += 2;
    }

    let mut first_frame_at: std::collections::HashMap<u16, usize> = std::collections::HashMap::new();
    for (f, frame) in frames.iter().enumerate() {
        if frame.sprites.is_empty() { continue; }

        let off = frame_offsets[f];
        if let Some(&orig) = first_frame_at.get(&off) {
            s.push_str(&format!("\n  // --- Frame {} : duplicate of frame {} (offset {}) ---\n",
                f, orig, off));
            continue;
        }
        first_frame_at.insert(off, f);

        let count = frame.sprites.len();
        s.push_str(&format!("\n  // --- Frame {} : {} sprites ---\n", f, count));

        s.push_str(&format!("  0x{:02x},        // count={}\n", bin[pos], count));
        pos += 1;

        // x_hi bytes
        let x_hi_bytes = 1 + if count > 8 { (count - 8 + 7) / 8 } else { 0 };
        for b in 0..x_hi_bytes {
            let label = if b == 0 { "x_hi_init" } else { &format!("x_hi_overflow[{}]", b - 1) };
            s.push_str(&format!("  0x{:02x},        // {} = {:08b}\n",
                bin[pos], label, bin[pos]));
            pos += 1;
        }

        // Sprite data
        for (i, &(x, y)) in frame.sprites.iter().enumerate() {
            let x_hi_bit = if x > 255 { 1 } else { 0 };
            let x_note = if x_hi_bit != 0 {
                format!(" (x_hi bit {}:{})", i / 8, i % 8)
            } else {
                String::new()
            };
            s.push_str(&format!("  0x{:02x}, 0x{:02x},  // y={} x={}{}\n",
                bin[pos], bin[pos + 1], y, x, x_note));
            pos += 2;
        }
    }

    s.push_str("};\n");
    s
}

// ---------------------------------------------------------------------------
// Frame uniqueness analysis
// ---------------------------------------------------------------------------

pub fn analyze_memory(prune_dist: f64, seq_frames: &[SeqFrame]) {
    use std::collections::HashMap;

    let stamp_frames = build_export_data(prune_dist, seq_frames);
    let sprite_frames = build_sprite_export_data(prune_dist, seq_frames);

    fn stamp_frame_data(frame: &ExportFrame) -> Vec<u8> {
        let single = serialize_binary(&[ExportFrame {
            streams: frame.streams.iter().map(|s| ExportStream {
                c64_color: s.c64_color,
                stamps: s.stamps.iter().map(|st| ExportStamp {
                    screen_offset: st.screen_offset,
                    stamp_index: st.stamp_index,
                }).collect(),
            }).collect(),
        }]);
        single[4..].to_vec()
    }

    fn sprite_frame_data(frame: &ExportSpriteFrame) -> Vec<u8> {
        if frame.sprites.is_empty() { return vec![]; }
        let single = serialize_sprite_binary(&[ExportSpriteFrame {
            sprites: frame.sprites.clone(),
        }]);
        single[4..].to_vec()
    }

    // Build per-frame combined data and deduplicate
    let mut seen: HashMap<Vec<u8>, u16> = HashMap::new(); // data -> unique index
    let mut unique_data: Vec<Vec<u8>> = Vec::new();
    let mut sequence: Vec<u16> = Vec::new(); // frame -> unique index
    let mut raw_total: usize = 0;

    for f in 0..TOTAL_FRAMES {
        let st = stamp_frame_data(&stamp_frames[f]);
        let sp = sprite_frame_data(&sprite_frames[f]);

        // Combined frame data: stamp data + separator + sprite data
        let mut combined = st;
        combined.push(0xFF); // separator
        combined.extend_from_slice(&sp);

        raw_total += combined.len() - 1; // subtract separator

        if let Some(&idx) = seen.get(&combined) {
            sequence.push(idx);
        } else {
            let idx = unique_data.len() as u16;
            seen.insert(combined.clone(), idx);
            unique_data.push(combined);
            sequence.push(idx);
        }
    }

    let unique_count = unique_data.len();
    let unique_bytes: usize = unique_data.iter().map(|d| d.len() - 1).sum(); // subtract separators
    let index_table = TOTAL_FRAMES * 2;
    let deduped_total = index_table + unique_bytes;

    // Per-frame size stats
    let frame_sizes: Vec<usize> = unique_data.iter().map(|d| d.len() - 1).collect();
    let max_frame = *frame_sizes.iter().max().unwrap_or(&0);
    let min_nonempty = frame_sizes.iter().filter(|&&s| s > 0).min().copied().unwrap_or(0);
    let avg_frame = if unique_count > 0 { unique_bytes / unique_count } else { 0 };

    eprintln!("=== Memory analysis (prune={:.1}) ===", prune_dist);
    eprintln!("  {} total frames, {} unique ({} dups)",
        TOTAL_FRAMES, unique_count, TOTAL_FRAMES - unique_count);
    eprintln!();
    eprintln!("  Raw (no dedup):      {:>6} bytes ({:.1} KB)", raw_total, raw_total as f64 / 1024.0);
    eprintln!("  Index table:         {:>6} bytes ({} frames x 2)", index_table, TOTAL_FRAMES);
    eprintln!("  Unique frame data:   {:>6} bytes ({:.1} KB)", unique_bytes, unique_bytes as f64 / 1024.0);
    eprintln!("  Deduped total:       {:>6} bytes ({:.1} KB)", deduped_total, deduped_total as f64 / 1024.0);
    eprintln!("  Savings:             {:>6} bytes ({:.1}%)",
        raw_total.saturating_sub(deduped_total),
        if raw_total > 0 { (1.0 - deduped_total as f64 / raw_total as f64) * 100.0 } else { 0.0 });
    eprintln!();
    eprintln!("  Frame sizes: min={} avg={} max={} bytes", min_nonempty, avg_frame, max_frame);
}

// ---------------------------------------------------------------------------
// Playlist export
// ---------------------------------------------------------------------------
//
// The playlist drives the C64 player (anim_handler): segments played in
// order, each repeated `repeats` times, wrapping to the first entry at the
// end. Offsets are byte offsets into the frame offset tables (frame * 2) and
// are shared by stamps.bin and sprites.bin (both are indexed by z_frame).
//
//   Byte 0: entry_count
//   Per entry (5 bytes):
//     start_lo, start_hi   z_frame offset of first frame   (start_frame * 2)
//     end_lo,   end_hi     z_frame offset past last frame  (end_frame * 2)
//     repeats              play count (0 = 256)

pub fn serialize_playlist(repeats: &[u16]) -> (Vec<u8>, String) {
    let mut bin: Vec<u8> = Vec::new();
    bin.push(SEGMENTS.len() as u8);

    let mut txt = String::new();
    let total_pres: usize = SEGMENTS.iter().enumerate()
        .map(|(i, s)| s.len * if s.loops { repeats[i].max(1) as usize } else { 1 })
        .sum();
    txt.push_str(&format!(
        "// C64 Playlist Export — {} entries, {} data frames, {} presented frames ({:.1}s at 50fps)\n",
        SEGMENTS.len(), TOTAL_FRAMES, total_pres, total_pres as f64 / FPS));
    txt.push_str("// Per entry: [start_lo, start_hi] [end_lo, end_hi] [repeats] — offsets are frame*2\n\n");
    txt.push_str("const unsigned char playlist_data[] = {\n");
    txt.push_str(&format!("  0x{:02x},        // entry_count={}\n", SEGMENTS.len(), SEGMENTS.len()));

    for (i, seg) in SEGMENTS.iter().enumerate() {
        let start = (segment_start(i) * 2) as u16;
        let end = ((segment_start(i) + seg.len) * 2) as u16;
        let reps = if seg.loops { repeats[i].clamp(1, 255) as u8 } else { 1 };
        bin.push((start & 0xFF) as u8);
        bin.push((start >> 8) as u8);
        bin.push((end & 0xFF) as u8);
        bin.push((end >> 8) as u8);
        bin.push(reps);
        txt.push_str(&format!(
            "  0x{:02x}, 0x{:02x}, 0x{:02x}, 0x{:02x}, 0x{:02x},  // {}: frames {}..{} x{}{}\n",
            start & 0xFF, start >> 8, end & 0xFF, end >> 8, reps,
            seg.name, segment_start(i), segment_start(i) + seg.len - 1, reps,
            if seg.loops { " [loop]" } else { "" }));
    }
    txt.push_str("};\n");

    (bin, txt)
}

pub fn export_playlist(repeats: &[u16]) -> (usize, usize) {
    let (bin, txt) = serialize_playlist(repeats);
    let sizes = (bin.len(), txt.len());
    std::fs::write("playlist.bin", &bin).expect("Failed to write playlist.bin");
    std::fs::write("playlist.txt", &txt).expect("Failed to write playlist.txt");
    sizes
}

// ---------------------------------------------------------------------------
// C64 memory budgets — must match prt_circleplotter.asm and spindle.txt:
// stamps.bin at $5800, sprites.bin at $9000, playlist.bin at $cf00.
// ---------------------------------------------------------------------------

pub const STAMPS_BUDGET: usize = 0x9000 - 0x5800;   // 14336
pub const SPRITES_BUDGET: usize = 0xcf00 - 0x9000;  // 16128
pub const PLAYLIST_BUDGET: usize = 0xd000 - 0xcf00; // 256

// ---------------------------------------------------------------------------
// Top-level export functions
// ---------------------------------------------------------------------------

pub fn export_stamps(prune_dist: f64, seq_frames: &[SeqFrame]) -> (usize, usize) {
    let frames = build_export_data(prune_dist, seq_frames);
    let bin = serialize_binary(&frames);
    let txt = serialize_text(&frames);

    let bin_size = bin.len();
    let txt_size = txt.len();

    std::fs::write("stamps.bin", &bin).expect("Failed to write stamps.bin");
    std::fs::write("stamps.txt", &txt).expect("Failed to write stamps.txt");

    (bin_size, txt_size)
}

pub fn export_sprites(prune_dist: f64, seq_frames: &[SeqFrame]) -> (usize, usize) {
    let frames = build_sprite_export_data(prune_dist, seq_frames);
    let bin = serialize_sprite_binary(&frames);
    let txt = serialize_sprite_text(&frames);

    let bin_size = bin.len();
    let txt_size = txt.len();

    std::fs::write("sprites.bin", &bin).expect("Failed to write sprites.bin");
    std::fs::write("sprites.txt", &txt).expect("Failed to write sprites.txt");

    (bin_size, txt_size)
}
