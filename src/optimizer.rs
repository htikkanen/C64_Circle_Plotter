use std::sync::{Arc, Mutex};

use rand::prelude::*;
use rand::rngs::SmallRng;

use crate::data::*;
use crate::render::{pixel_error, render_c64_image, render_ideal};
use crate::sim::*;

// ---------------------------------------------------------------------------
// Optimizer state
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct OptState {
    pub modes: Vec<DiscMode>,
    pub asgn: Vec<Assignment>,
    pub sprite_slot_map: Vec<Option<u8>>,
}

#[derive(Clone)]
pub struct OptProgress {
    pub iterations_done: u64,
    pub iterations_total: u64,
    pub best_score: u32,
    pub baseline_score: u32,
    pub done: bool,
    pub best_state: Option<OptState>,
    pub sprites_demoted: u32,
}

// ---------------------------------------------------------------------------
// Build OptState from current allocator output
// ---------------------------------------------------------------------------

pub fn state_from_alloc(asgn: &[Assignment], sprite_slot_map: &[Option<u8>]) -> OptState {
    OptState {
        modes: asgn.iter().map(|a| a.mode).collect(),
        asgn: asgn.to_vec(),
        sprite_slot_map: sprite_slot_map.to_vec(),
    }
}

// ---------------------------------------------------------------------------
// Evaluate: render C64 image from state and score against ideal
// ---------------------------------------------------------------------------

/// Raw pixel error for display/reporting.
fn raw_error(state: &OptState, ideal: &[u8], glitch_color_active: bool, glitch_frame: usize) -> u32 {
    let actual = render_c64_image(&state.asgn, &state.sprite_slot_map, glitch_color_active, glitch_frame);
    pixel_error(&actual, ideal)
}

/// Penalized score for optimization: pixel error * 100 + sprite count.
/// Small sprite penalty discourages unnecessary promotions.
fn evaluate(state: &OptState, ideal: &[u8], glitch_color_active: bool, glitch_frame: usize) -> u32 {
    let error = raw_error(state, ideal, glitch_color_active, glitch_frame);
    let sprite_count = state.asgn.iter().filter(|a| a.mode == DiscMode::Sprite).count() as u32;
    error * 100 + sprite_count
}

// ---------------------------------------------------------------------------
// Rebuild sprite slot map from modes
// ---------------------------------------------------------------------------

fn rebuild_sprite_slots(asgn: &[Assignment]) -> Vec<Option<u8>> {
    struct SprInfo { idx: usize, top_y: i32 }
    let mut infos: Vec<SprInfo> = Vec::new();
    for (i, a) in asgn.iter().enumerate() {
        if a.mode != DiscMode::Sprite { continue; }
        infos.push(SprInfo { idx: i, top_y: (a.y.floor() as i32) - 8 });
    }
    infos.sort_by_key(|s| s.top_y);

    let mut slot_free_at = [-999i32; 8];
    let mut map = vec![None; asgn.len()];
    for si in &infos {
        for s in 0..8u8 {
            if slot_free_at[s as usize] <= si.top_y {
                slot_free_at[s as usize] = si.top_y + MUX_H as i32;
                map[si.idx] = Some(s);
                break;
            }
        }
    }
    map
}

// ---------------------------------------------------------------------------
// Mutations
// ---------------------------------------------------------------------------

/// Try promoting a disc to sprite, returns true if successful.
fn try_promote(state: &mut OptState, idx: usize) -> bool {
    if state.asgn[idx].mode != DiscMode::Char { return false; }
    let mut test_list: Vec<(f64, f64)> = state.asgn.iter()
        .enumerate()
        .filter(|(i, a)| a.mode == DiscMode::Sprite && *i != idx)
        .map(|(_, a)| (a.x, a.y))
        .collect();
    test_list.push((state.asgn[idx].x, state.asgn[idx].y));
    if try_mux_fit(&test_list) {
        state.asgn[idx].mode = DiscMode::Sprite;
        state.modes[idx] = DiscMode::Sprite;
        state.sprite_slot_map = rebuild_sprite_slots(&state.asgn);
        true
    } else {
        false
    }
}

fn mutate(state: &mut OptState, rng: &mut SmallRng) -> bool {
    let n = state.asgn.len();
    if n == 0 { return false; }

    let choice = rng.random_range(0..100u32);

    if choice < 30 {
        // Random char <-> sprite swap
        let idx = rng.random_range(0..n);
        if state.asgn[idx].mode == DiscMode::Offscreen { return false; }
        match state.asgn[idx].mode {
            DiscMode::Char => try_promote(state, idx),
            DiscMode::Sprite => {
                if state.asgn[idx].stamp.is_empty() { return false; }
                let (_, clipped) = get_stamp_cells_with_clip(state.asgn[idx].x, state.asgn[idx].y);
                if clipped { return false; } // don't demote clipped stamps
                state.asgn[idx].mode = DiscMode::Char;
                state.modes[idx] = DiscMode::Char;
                state.sprite_slot_map = rebuild_sprite_slots(&state.asgn);
                true
            }
            _ => false,
        }
    } else if choice < 50 {
        // Greedy: promote ALL char discs that fit in mux
        let mut any = false;
        let chars: Vec<usize> = (0..n)
            .filter(|&i| state.asgn[i].mode == DiscMode::Char)
            .collect();
        for idx in chars {
            if try_promote(state, idx) { any = true; }
        }
        any
    } else if choice < 70 {
        // Swap write order between two char discs
        let chars: Vec<usize> = (0..n)
            .filter(|&i| state.asgn[i].mode == DiscMode::Char)
            .collect();
        if chars.len() < 2 { return false; }
        let a = chars[rng.random_range(0..chars.len())];
        let b = chars[rng.random_range(0..chars.len())];
        if a == b { return false; }
        state.asgn.swap(a, b);
        state.modes.swap(a, b);
        state.sprite_slot_map.swap(a, b);
        true
    } else if choice < 85 {
        // Nudge a non-ghost char disc by 1-2 pixels
        let chars: Vec<usize> = (0..n)
            .filter(|&i| state.asgn[i].mode == DiscMode::Char && !state.asgn[i].is_ghost)
            .collect();
        if chars.is_empty() { return false; }
        let idx = chars[rng.random_range(0..chars.len())];
        let dx = rng.random_range(-2..=2i32) as f64;
        let dy = rng.random_range(-2..=2i32) as f64;
        if dx == 0.0 && dy == 0.0 { return false; }
        let new_x = state.asgn[idx].x + dx;
        let new_y = state.asgn[idx].y + dy;
        let new_stamp = get_stamp_cells(new_x, new_y);
        if new_stamp.is_empty() { return false; }
        state.asgn[idx].x = new_x;
        state.asgn[idx].y = new_y;
        state.asgn[idx].stamp = new_stamp;
        true
    } else if choice < 95 {
        // Toggle ghost offscreen
        let ghosts: Vec<usize> = (0..n)
            .filter(|&i| state.asgn[i].is_ghost && state.asgn[i].mode != DiscMode::Offscreen)
            .collect();
        if ghosts.is_empty() { return false; }
        let idx = ghosts[rng.random_range(0..ghosts.len())];
        state.asgn[idx].mode = DiscMode::Offscreen;
        state.modes[idx] = DiscMode::Offscreen;
        state.sprite_slot_map = rebuild_sprite_slots(&state.asgn);
        true
    } else {
        // Demote a random sprite to char, then re-promote greedily
        // (reshuffles which discs get sprite slots)
        let sprites: Vec<usize> = (0..n)
            .filter(|&i| state.asgn[i].mode == DiscMode::Sprite)
            .collect();
        if sprites.is_empty() { return false; }
        let idx = sprites[rng.random_range(0..sprites.len())];
        if state.asgn[idx].stamp.is_empty() { return false; }
        state.asgn[idx].mode = DiscMode::Char;
        state.modes[idx] = DiscMode::Char;
        state.sprite_slot_map = rebuild_sprite_slots(&state.asgn);
        // Now greedily re-promote
        let chars: Vec<usize> = (0..n)
            .filter(|&i| state.asgn[i].mode == DiscMode::Char)
            .collect();
        for ci in chars { try_promote(state, ci); }
        true
    }
}

// ---------------------------------------------------------------------------
// Single-thread hill climbing
// ---------------------------------------------------------------------------

#[allow(dead_code)]
fn optimize_single(
    ideal: &[u8],
    initial: OptState,
    iterations: u64,
    seed: u64,
    glitch_color_active: bool,
    glitch_frame: usize,
) -> (OptState, u32) {
    let mut rng = SmallRng::seed_from_u64(seed);
    let mut current = initial;
    let mut current_score = evaluate(&current, ideal, glitch_color_active, glitch_frame);

    for _ in 0..iterations {
        let mut candidate = current.clone();
        if !mutate(&mut candidate, &mut rng) {
            continue;
        }
        let score = evaluate(&candidate, ideal, glitch_color_active, glitch_frame);
        if score < current_score {
            current = candidate;
            current_score = score;
        }
    }
    (current, current_score)
}

// ---------------------------------------------------------------------------
// Parallel optimizer — runs on background threads, updates progress
// ---------------------------------------------------------------------------

pub fn optimize_parallel(
    vis_positions: Vec<DiscPosition>,
    initial: OptState,
    glitch_color_active: bool,
    glitch_frame: usize,
    iterations_per_thread: u64,
    progress: Arc<Mutex<OptProgress>>,
) {
    let n_threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);

    let ideal = render_ideal(&vis_positions, glitch_color_active, glitch_frame);

    {
        let mut p = progress.lock().unwrap();
        // baseline_score and best_score already set by UI thread (raw pixel error)
        p.iterations_total = iterations_per_thread * n_threads as u64;
    }

    let ideal = Arc::new(ideal);
    let initial = Arc::new(initial);

    std::thread::scope(|s| {
        let handles: Vec<_> = (0..n_threads)
            .map(|t| {
                let ideal = Arc::clone(&ideal);
                let init = (*initial).clone();
                let progress = Arc::clone(&progress);
                s.spawn(move || {
                    let mut rng = SmallRng::seed_from_u64(t as u64 * 12345 + 67);
                    let mut current = init;
                    let mut current_score = evaluate(&current, &ideal, glitch_color_active, glitch_frame);
                    let report_interval = (iterations_per_thread / 20).max(1);

                    for i in 0..iterations_per_thread {
                        let mut candidate = current.clone();
                        if mutate(&mut candidate, &mut rng) {
                            let score = evaluate(&candidate, &ideal, glitch_color_active, glitch_frame);
                            if score < current_score {
                                current = candidate;
                                current_score = score;
                            }
                        }
                        if i % report_interval == 0 {
                            let mut p = progress.lock().unwrap();
                            p.iterations_done += report_interval;
                            let raw = raw_error(&current, &ideal, glitch_color_active, glitch_frame);
                            if raw < p.best_score {
                                p.best_score = raw;
                            }
                        }
                    }
                    (current, current_score)
                })
            })
            .collect();

        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        let best = results.into_iter().min_by_key(|(_, score)| *score).unwrap();

        // Final cleanup: demote unnecessary sprites to chars
        let (cleaned, demoted) = demote_unnecessary_sprites(best.0, &ideal, glitch_color_active, glitch_frame);

        let mut p = progress.lock().unwrap();
        p.best_score = raw_error(&cleaned, &ideal, glitch_color_active, glitch_frame);
        p.best_state = Some(cleaned);
        p.sprites_demoted = demoted;
        p.done = true;
        p.iterations_done = p.iterations_total;
    });
}

/// Try demoting each sprite to char. Keep demotion if pixel error doesn't increase.
/// Returns (optimized state, number of demotions).
fn demote_unnecessary_sprites(
    mut state: OptState,
    ideal: &[u8],
    glitch_color_active: bool,
    glitch_frame: usize,
) -> (OptState, u32) {
    let base_error = raw_error(&state, ideal, glitch_color_active, glitch_frame);
    let mut demoted = 0u32;

    let sprites: Vec<usize> = (0..state.asgn.len())
        .filter(|&i| state.asgn[i].mode == DiscMode::Sprite && !state.asgn[i].stamp.is_empty())
        .collect();

    for idx in sprites {
        // Don't demote if stamp would be viewport-clipped
        let (_, clipped) = get_stamp_cells_with_clip(state.asgn[idx].x, state.asgn[idx].y);
        if clipped { continue; }

        state.asgn[idx].mode = DiscMode::Char;
        state.modes[idx] = DiscMode::Char;
        state.sprite_slot_map = rebuild_sprite_slots(&state.asgn);

        let new_error = raw_error(&state, ideal, glitch_color_active, glitch_frame);
        if new_error > base_error {
            state.asgn[idx].mode = DiscMode::Sprite;
            state.modes[idx] = DiscMode::Sprite;
            state.sprite_slot_map = rebuild_sprite_slots(&state.asgn);
        } else {
            demoted += 1;
        }
    }
    (state, demoted)
}
