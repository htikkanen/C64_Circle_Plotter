use crate::data::*;
use crate::SeqFrame;
use crate::render::shade_index;
use crate::sim::*;

// ---------------------------------------------------------------------------
// C64 color mapping — 1-bit class model
// ---------------------------------------------------------------------------
// Every frame uses two colors. Specular segments: purple base + highlight
// band (SpecularParams.c1) that tracks the camera/wobble. Trail segments:
// blue ghosts/far discs + purple main discs. Glitch is a sprite-only effect;
// char colors are glitch-free.

pub const COL_BASE: u8 = 4; // purple — also the C64 color RAM init fill
pub const COL_GHOST: u8 = 6; // blue

pub const C64_COLOR_NAMES: [&str; 16] = [
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

pub fn build_export_data(
    prune_dist: f64,
    seq_frames: &[SeqFrame],
    spec: &SpecularParams,
) -> Vec<ExportFrame> {
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

        // Collect char stamps with their 1-bit class color. `rank` orders the
        // streams: the rank-1 stream paints last and wins shared cells
        // (highlight over base; main discs over ghosts).
        struct StampEntry {
            rank: u8,
            c64_color: u8,
            eff_z: f64,
            screen_offset: u16,
            stamp_index: u8,
        }

        let mut entries: Vec<StampEntry> = Vec::new();

        for a in &asgn {
            if a.mode != DiscMode::Char { continue; }

            let spec_u = if spec.enabled {
                specular_u(f, a.x, a.y, spec)
            } else {
                None
            };
            let (rank, c64_color) = if let Some(u) = spec_u {
                if specular_lit(u, spec) {
                    (1, spec.c1)
                } else {
                    (0, COL_BASE)
                }
            } else {
                // trail segments: mono chars — the color pass is skipped
                // there (playlist flag) and trail color rides on sprites
                (0, COL_BASE)
            };
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
                rank,
                c64_color,
                eff_z,
                screen_offset,
                stamp_index: stamp_idx,
            });
        }

        // Order by paint rank, back-to-front within each rank
        entries.sort_by(|a, b| {
            a.rank.cmp(&b.rank)
                .then(b.eff_z.total_cmp(&a.eff_z)) // back first within group
        });

        // Build streams grouped by C64 color
        let mut streams: Vec<ExportStream> = Vec::new();
        let mut i = 0;
        while i < entries.len() {
            let c64_col = entries[i].c64_color;
            let mut stamps = Vec::new();
            while i < entries.len() && entries[i].c64_color == c64_col {
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
// Sprite export (v4 flat format — 1-bit color class per sprite)
// ===========================================================================

pub(crate) struct ExportSpriteFrame {
    sprites: Vec<(u16, u8, bool)>, // (x, y, class1) VIC coords, Y-sorted
    color0: u8, // bits 0-3: C64 color, bit 7: class-0 sprites behind chars
    color1: u8, // same for class 1
}

pub fn build_sprite_export_data(
    prune_dist: f64,
    seq_frames: &[SeqFrame],
    spec: &SpecularParams,
) -> Vec<ExportSpriteFrame> {
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

        // Frame color pair — mirrors the char stream colors: specular
        // segments are base + highlight (all in front), trail segments are
        // main + far/ghost class rendered behind chars (bit 7).
        let is_spec = spec.enabled && segment_is_specular(segment_at(f).0);
        let (color0, color1) = if is_spec {
            (COL_BASE, spec.c1 & 15)
        } else {
            (COL_BASE, COL_GHOST | 0x80)
        };

        let mut sprites: Vec<(u16, u8, bool)> = Vec::new();

        // Ghost sprites (trail tails) export too — the allocator reserved
        // mux slots for them, and they carry the trail color (class 1,
        // blue behind chars) now that trail chars are mono. Frames over the
        // mux budget shed their deepest tails first.
        let dropped = capped_sprite_drops(&asgn, &sprite_slot_map);
        for (ai, a) in asgn.iter().enumerate() {
            if a.mode != DiscMode::Sprite { continue; }
            if sprite_slot_map.get(ai).and_then(|s| *s).is_none() { continue; }
            if dropped[ai] { continue; }

            let class1 = if is_spec {
                specular_u(f, a.x, a.y, spec)
                    .map(|u| specular_lit(u, spec))
                    .unwrap_or(false)
            } else {
                shade_index(a, false, f) <= 2
            };

            let ox = a.x.floor() as i32 - 8;
            let oy = a.y.floor() as i32 - 8;

            let x = (ox + 24).max(0) as u16;
            let y = (oy + 51).clamp(0, 255) as u8;

            sprites.push((x, y, class1));
        }

        sprites.sort_by_key(|&(_, y, _)| y);

        frames.push(ExportSpriteFrame { sprites, color0, color1 });
    }

    frames
}

// ---------------------------------------------------------------------------
// Sprite binary serialization (v4 flat format)
// ---------------------------------------------------------------------------
//
// Header:
//   [num_frames: u16]
//   [frame_offsets: u16 × num_frames]  (0xFFFF = empty frame)
//
// Per frame:
//   [total_count: u8]
//   [color0: u8]                 bits 0-3: C64 color of class-0 sprites,
//                                bit 7: class-0 sprites behind chars ($D01B)
//   [color1: u8]                 same for class-1 sprites
//   [x_hi_init: u8]              x_hi bits for sprites 0-7
//   [x_hi_overflow: u8 × N]     ceil((count-8)/8) bytes, 0 if count <= 8
//   [class_init: u8]             class bits for sprites 0-7
//   [class_overflow: u8 × N]    same packing as x_hi
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
            fd.push(frame.color0);
            fd.push(frame.color1);

            // Bitmask packing: first byte covers sprites 0-7, then one byte
            // per 8 additional — same layout for x_hi and class bits
            let mask_bytes = 1 + if count > 8 { (count - 8 + 7) / 8 } else { 0 };
            let mut x_hi = vec![0u8; mask_bytes];
            let mut class = vec![0u8; mask_bytes];
            for (i, &(x, _, c1)) in frame.sprites.iter().enumerate() {
                if x > 255 {
                    x_hi[i / 8] |= 1 << (i % 8);
                }
                if c1 {
                    class[i / 8] |= 1 << (i % 8);
                }
            }
            fd.extend_from_slice(&x_hi);
            fd.extend_from_slice(&class);

            // Sprite data: [y, x_lo] per sprite
            for &(x, y, _) in &frame.sprites {
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
    s.push_str(&format!("// C64 Sprite Export v4 — {} frames, {} bytes\n", num_frames, bin.len()));
    s.push_str("// Per frame: [count] [color0] [color1] [x_hi...] [class...] [y, x_lo]...\n");
    s.push_str("// color bytes: bits 0-3 C64 color, bit 7 = class rendered behind chars\n\n");

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

        // Frame color pair
        for (label, col) in [("color0", frame.color0), ("color1", frame.color1)] {
            s.push_str(&format!("  0x{:02x},        // {} = {} ({}){}\n",
                bin[pos], label, col & 15,
                C64_COLOR_NAMES[(col & 15) as usize],
                if col & 0x80 != 0 { " [behind chars]" } else { "" }));
            pos += 1;
        }

        // x_hi + class bitmask bytes
        let mask_bytes = 1 + if count > 8 { (count - 8 + 7) / 8 } else { 0 };
        for (name, _) in [("x_hi", 0), ("class", 1)] {
            for b in 0..mask_bytes {
                let label = if b == 0 {
                    format!("{}_init", name)
                } else {
                    format!("{}_overflow[{}]", name, b - 1)
                };
                s.push_str(&format!("  0x{:02x},        // {} = {:08b}\n",
                    bin[pos], label, bin[pos]));
                pos += 1;
            }
        }

        // Sprite data
        for (i, &(x, y, c1)) in frame.sprites.iter().enumerate() {
            let mut notes = String::new();
            if x > 255 {
                notes.push_str(&format!(" (x_hi bit {}:{})", i / 8, i % 8));
            }
            if c1 {
                notes.push_str(" [class1]");
            }
            s.push_str(&format!("  0x{:02x}, 0x{:02x},  // y={} x={}{}\n",
                bin[pos], bin[pos + 1], y, x, notes));
            pos += 2;
        }
    }

    s.push_str("};\n");
    s
}

// ---------------------------------------------------------------------------
// Frame uniqueness analysis
// ---------------------------------------------------------------------------

pub fn analyze_memory(prune_dist: f64, seq_frames: &[SeqFrame], spec: &SpecularParams) {
    use std::collections::HashMap;

    let stamp_frames = build_export_data(prune_dist, seq_frames, spec);
    let sprite_frames = build_sprite_export_data(prune_dist, seq_frames, spec);

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
            color0: frame.color0,
            color1: frame.color1,
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
//   Per entry (6 bytes):
//     start_lo, start_hi   z_frame offset of first frame   (start_frame * 2)
//     end_lo,   end_hi     z_frame offset past last frame  (end_frame * 2)
//     repeats              play count (0 = 256)
//     flags                bit 0: skip the color pass in this segment

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
    txt.push_str("// Per entry: [start_lo, start_hi] [end_lo, end_hi] [repeats] [flags]\n");
    txt.push_str("// offsets are frame*2; flags bit 0 = skip color pass\n\n");
    txt.push_str("const unsigned char playlist_data[] = {\n");
    txt.push_str(&format!("  0x{:02x},        // entry_count={}\n", SEGMENTS.len(), SEGMENTS.len()));

    for (i, seg) in SEGMENTS.iter().enumerate() {
        let start = (segment_start(i) * 2) as u16;
        let end = ((segment_start(i) + seg.len) * 2) as u16;
        let reps = if seg.loops { repeats[i].clamp(1, 255) as u8 } else { 1 };
        let flags = if seg.color_pass { 0u8 } else { 1u8 };
        bin.push((start & 0xFF) as u8);
        bin.push((start >> 8) as u8);
        bin.push((end & 0xFF) as u8);
        bin.push((end >> 8) as u8);
        bin.push(reps);
        bin.push(flags);
        txt.push_str(&format!(
            "  0x{:02x}, 0x{:02x}, 0x{:02x}, 0x{:02x}, 0x{:02x}, 0x{:02x},  // {}: frames {}..{} x{}{}{}\n",
            start & 0xFF, start >> 8, end & 0xFF, end >> 8, reps, flags,
            seg.name, segment_start(i), segment_start(i) + seg.len - 1, reps,
            if seg.loops { " [loop]" } else { "" },
            if seg.color_pass { "" } else { " [no color pass]" }));
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
// Color RAM plan (hw-true preview, mirrors the C64 color pass)
// ---------------------------------------------------------------------------
//
// Models the C64 writer: color RAM is initialized once to COL_BASE and never
// cleared. Each frame, a clear-shaped color pass repaints every stamp's cells
// with its stream color in the bottom-border window (before the char flip at
// line 0), walking the about-to-flip frame's data. Full repaint = stateless:
// covered cells always show the current frame's color; uncovered cells keep
// stale values invisibly. Mono segments skip the pass entirely.

/// Approximate color-pass cost per stamp: ~50 cycles parse/dispatch plus
/// ~8 per non-empty cell (avg 11 cells in the circle stamp set).
pub const COLOR_PASS_CYCLES_PER_STAMP: usize = 140;

pub struct ColramPlan {
    pub states: Vec<Vec<u8>>, // per presentation step: color RAM after the pass
    pub stamps: Vec<u16>,     // color-pass stamps per step (CPU cost)
    pub changed: Vec<u16>,    // cells whose displayed color actually changed
    pub prune_dist: f64,
}

pub fn build_colram_plan(
    prune_dist: f64,
    seq_frames: &[SeqFrame],
    spec: &SpecularParams,
    seg_colored: &[bool],
    pres_map: &[u32],
) -> ColramPlan {
    let frames = build_export_data(prune_dist, seq_frames, spec);
    let cells = ROWS * COLS;
    let stamps_table = &*STAMPS_TABLE;

    // Per data frame: coverage + color per cell. Replaying the streams in
    // export order matches the C64 paint order, so the last writer of a
    // shared cell wins — same rule for chars and colors.
    let mut covered: Vec<Vec<bool>> = Vec::with_capacity(frames.len());
    let mut req: Vec<Vec<u8>> = Vec::with_capacity(frames.len());
    for frame in &frames {
        let mut cov = vec![false; cells];
        let mut rc = vec![COL_BASE; cells];
        for stream in &frame.streams {
            for stamp in &stream.stamps {
                for sc in &stamps_table[stamp.stamp_index as usize] {
                    let cell = stamp.screen_offset as usize
                        + sc.dr as usize * COLS + sc.dc as usize;
                    if cell < cells {
                        cov[cell] = true;
                        rc[cell] = stream.c64_color;
                    }
                }
            }
        }
        covered.push(cov);
        req.push(rc);
    }

    // Replay the full-repaint pass over the presentation (loops included).
    let mut ram = vec![COL_BASE; cells];
    let mut states = Vec::with_capacity(pres_map.len());
    let mut stamps = Vec::with_capacity(pres_map.len());
    let mut changed = Vec::with_capacity(pres_map.len());

    for &pf in pres_map {
        let f = pf as usize;
        let (seg, _) = segment_at(f);
        let mut st = 0usize;
        let mut ch = 0usize;
        if seg_colored.get(seg).copied().unwrap_or(false) {
            st = frames[f].streams.iter().map(|s| s.stamps.len()).sum();
            for c in 0..cells {
                if covered[f][c] && ram[c] != req[f][c] {
                    ram[c] = req[f][c];
                    ch += 1;
                }
            }
        }
        states.push(ram.clone());
        stamps.push(st as u16);
        changed.push(ch as u16);
    }

    ColramPlan { states, stamps, changed, prune_dist }
}

// ---------------------------------------------------------------------------
// C64 memory budgets — must match prt_circleplotter.asm and spindle.txt:
// stamps.bin at $5800, sprites.bin at $9000, playlist.bin at $0400
// (unused default screen RAM; VIC runs from the $4000 bank).
// ---------------------------------------------------------------------------

pub const STAMPS_BUDGET: usize = 0x9000 - 0x5800;   // 14336
pub const SPRITES_BUDGET: usize = 0xd000 - 0x9000;  // 16384
pub const PLAYLIST_BUDGET: usize = 0x0600 - 0x0400; // 512

// ---------------------------------------------------------------------------
// Top-level export functions
// ---------------------------------------------------------------------------

pub fn export_stamps(
    prune_dist: f64,
    seq_frames: &[SeqFrame],
    spec: &SpecularParams,
) -> (usize, usize) {
    let frames = build_export_data(prune_dist, seq_frames, spec);
    let bin = serialize_binary(&frames);
    let txt = serialize_text(&frames);

    let bin_size = bin.len();
    let txt_size = txt.len();

    std::fs::write("stamps.bin", &bin).expect("Failed to write stamps.bin");
    std::fs::write("stamps.txt", &txt).expect("Failed to write stamps.txt");

    (bin_size, txt_size)
}

pub fn export_sprites(
    prune_dist: f64,
    seq_frames: &[SeqFrame],
    spec: &SpecularParams,
) -> (usize, usize) {
    let frames = build_sprite_export_data(prune_dist, seq_frames, spec);
    let bin = serialize_sprite_binary(&frames);
    let txt = serialize_sprite_text(&frames);

    let bin_size = bin.len();
    let txt_size = txt.len();

    std::fs::write("sprites.bin", &bin).expect("Failed to write sprites.bin");
    std::fs::write("sprites.txt", &txt).expect("Failed to write sprites.txt");

    (bin_size, txt_size)
}
