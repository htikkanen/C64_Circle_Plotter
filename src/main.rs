mod data;
mod sim;
mod render;
mod optimizer;

use std::sync::{Arc, Mutex};
use eframe::egui;

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------
#[derive(Clone, Default)]
struct SeqFrame {
    result: Option<optimizer::OptState>,
    baseline: u32,
}

struct C64App {
    frame: usize,
    playing: bool,
    speed: f32,
    opts: render::DisplayOpts,
    corrupt_total: u32,
    last_corrupt_frame: Option<usize>,
    accum_mem_bytes: usize,

    // Rendering state
    texture: Option<egui::TextureHandle>,

    // Timing
    last_time: Option<f64>,
    accum: f64,

    // Optimizer
    opt_progress: Option<Arc<Mutex<optimizer::OptProgress>>>,
    opt_frame: Option<usize>,
    opt_iterations: u64,
    opt_compare: bool, // true = temporarily show baseline

    // Sequence optimizer
    seq_frames: Vec<SeqFrame>,
    seq_running: bool,
    seq_current_frame: usize,
    seq_total_saved: u32,
}

impl C64App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut style = (*cc.egui_ctx.style()).clone();
        style.visuals.dark_mode = true;
        style.visuals.panel_fill = COL_BG;
        style.visuals.window_fill = COL_PANEL;
        style.visuals.widgets.noninteractive.bg_fill = COL_PANEL;
        style.visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, COL_TEXT);
        cc.egui_ctx.set_style(style);
        Self {
            frame: 0,
            playing: true,
            speed: 1.0,
            opts: render::DisplayOpts::default(),
            corrupt_total: 0,
            last_corrupt_frame: None,
            accum_mem_bytes: 0,
            texture: None,
            last_time: None,
            accum: 0.0,
            opt_progress: None,
            opt_frame: None,
            opt_iterations: 5000,
            opt_compare: false,
            seq_frames: vec![SeqFrame::default(); data::TOTAL_FRAMES],
            seq_running: false,
            seq_current_frame: 0,
            seq_total_saved: 0,
        }
    }

    fn advance_frame(&mut self) {
        self.frame += 1;
        if self.frame >= data::TOTAL_FRAMES {
            self.frame = 0;
        }
    }

    fn reset(&mut self) {
        self.frame = 0;
        self.playing = false;
        self.corrupt_total = 0;
        self.last_corrupt_frame = None;
        self.accum_mem_bytes = 0;
        self.last_time = None;
        self.accum = 0.0;
    }

    /// Render the current frame and return the pixel buffer + stats.
    fn render_current_frame(&mut self) -> (Vec<u8>, render::FrameStats, Vec<u8>) {
        // Reset cumulative counters at frame 0
        if self.frame == 0 {
            self.corrupt_total = 0;
            self.accum_mem_bytes = 0;
        }

        let positions = sim::gen_positions(self.frame);

        // Use optimized allocation if available for this frame
        let opt_override = self.get_opt_override();
        let override_ref = opt_override.as_ref().map(|(a, s)| (a.as_slice(), s.as_slice()));

        let (pixels, stats, sl_counts) = render::render_frame(
            &positions,
            &self.opts,
            override_ref,
        );

        // Update counters (only once per frame — avoids inflation on repaints)
        if self.last_corrupt_frame != Some(self.frame) {
            self.corrupt_total += stats.conflicts;
            self.accum_mem_bytes += stats.mem_bytes;
            self.last_corrupt_frame = Some(self.frame);
        }

        (pixels, stats, sl_counts)
    }

    /// Get optimized allocation for current frame, if available.
    fn get_opt_override(&self) -> Option<(Vec<sim::Assignment>, Vec<Option<u8>>)> {
        if self.opt_compare { return None; } // compare mode: show baseline
        // Check single-frame optimization
        if self.opt_frame == Some(self.frame) {
            if let Some(ref progress) = self.opt_progress {
                let p = progress.lock().unwrap();
                if p.done {
                    if let Some(ref state) = p.best_state {
                        return Some((state.asgn.clone(), state.sprite_slot_map.clone()));
                    }
                }
            }
        }
        // Check sequence optimization
        if let Some(ref state) = self.seq_frames[self.frame].result {
            return Some((state.asgn.clone(), state.sprite_slot_map.clone()));
        }
        None
    }

    fn start_optimizer(&mut self) {
        self.playing = false;
        let frame = self.frame;
        let positions = sim::gen_positions(frame);

        let mut vis_positions: Vec<sim::DiscPosition> = positions.positions.iter()
            .filter(|p| !sim::should_skip(p.z))
            .cloned()
            .collect();
        sim::prune_by_proximity(&mut vis_positions, self.opts.prune_dist);

        // Allocate on pruned positions (same set as render_frame uses)
        let alloc = sim::allocate(&vis_positions);
        let initial = optimizer::state_from_alloc(&alloc.asgn, &alloc.sprite_slot_map);

        // Compute baseline error on UI thread to avoid race
        let baseline = {
            let ideal = render::render_ideal(&vis_positions, positions.glitch_color_active, positions.glitch_frame);
            let actual = render::render_c64_image(&initial.asgn, &initial.sprite_slot_map, positions.glitch_color_active, positions.glitch_frame);
            render::pixel_error(&actual, &ideal)
        };

        let progress = Arc::new(Mutex::new(optimizer::OptProgress {
            iterations_done: 0,
            iterations_total: 0,
            best_score: baseline,
            baseline_score: baseline,
            done: false,
            best_state: None,
            sprites_demoted: 0,
        }));
        self.opt_progress = Some(Arc::clone(&progress));
        self.opt_frame = Some(frame);

        let iters = self.opt_iterations;
        let glitch_color = positions.glitch_color_active;
        let glitch_frame_val = positions.glitch_frame;

        std::thread::spawn(move || {
            optimizer::optimize_parallel(
                vis_positions,
                initial,
                glitch_color,
                glitch_frame_val,
                iters,
                progress,
            );
        });
    }

    fn start_sequence_optimizer(&mut self) {
        self.playing = false;
        self.seq_running = true;
        self.seq_current_frame = 0;
        self.seq_total_saved = 0;
        self.seq_frames = vec![SeqFrame::default(); data::TOTAL_FRAMES];
        self.advance_sequence_optimizer();
    }

    fn advance_sequence_optimizer(&mut self) {
        if !self.seq_running || self.seq_current_frame >= data::TOTAL_FRAMES {
            self.seq_running = false;
            return;
        }

        let frame = self.seq_current_frame;
        let positions = sim::gen_positions(frame);
        let prune_dist = self.opts.prune_dist;
        let iters = self.opt_iterations;

        let mut vis_positions: Vec<sim::DiscPosition> = positions.positions.iter()
            .filter(|p| !sim::should_skip(p.z))
            .cloned()
            .collect();
        sim::prune_by_proximity(&mut vis_positions, prune_dist);

        let alloc = sim::allocate(&vis_positions);
        let initial = optimizer::state_from_alloc(&alloc.asgn, &alloc.sprite_slot_map);

        let baseline = {
            let ideal = render::render_ideal(&vis_positions, positions.glitch_color_active, positions.glitch_frame);
            let actual = render::render_c64_image(&initial.asgn, &initial.sprite_slot_map, positions.glitch_color_active, positions.glitch_frame);
            render::pixel_error(&actual, &ideal)
        };

        let progress = Arc::new(Mutex::new(optimizer::OptProgress {
            iterations_done: 0,
            iterations_total: 0,
            best_score: baseline,
            baseline_score: baseline,
            done: false,
            best_state: None,
            sprites_demoted: 0,
        }));
        self.opt_progress = Some(Arc::clone(&progress));
        self.opt_frame = Some(frame);
        self.frame = frame;

        let glitch_color = positions.glitch_color_active;
        let glitch_frame_val = positions.glitch_frame;

        std::thread::spawn(move || {
            optimizer::optimize_parallel(
                vis_positions,
                initial,
                glitch_color,
                glitch_frame_val,
                iters,
                progress,
            );
        });
    }
}

// ---------------------------------------------------------------------------
// Color constants
// ---------------------------------------------------------------------------
/// Phase metadata: (start_frame, end_frame, color, label)
const PHASES: [(usize, usize, egui::Color32, &str); 4] = [
    (0, data::P1_END, egui::Color32::from_rgb(0x44, 0x33, 0x55), "E zooms in"),
    (data::P1_END, data::P2_END, egui::Color32::from_rgb(0x33, 0x55, 0x44), "XTEND appears"),
    (data::P2_END, data::P3_END, egui::Color32::from_rgb(0x33, 0x44, 0x55), "E->D pan"),
    (data::P3_END, data::P4_END, egui::Color32::from_rgb(0x55, 0x33, 0x44), "exit"),
];

// Theme colors
const COL_BG: egui::Color32 = egui::Color32::from_rgb(0x0a, 0x0a, 0x0e);
const COL_PANEL: egui::Color32 = egui::Color32::from_rgb(0x11, 0x11, 0x18);
const COL_TEXT: egui::Color32 = egui::Color32::from_rgb(0xc0, 0xc0, 0xd0);
const COL_DIM: egui::Color32 = egui::Color32::from_rgb(0x66, 0x68, 0x88);
const COL_ACCENT: egui::Color32 = egui::Color32::from_rgb(0x6c, 0x6c, 0xff);
const COL_SPRITE: egui::Color32 = egui::Color32::from_rgb(0xff, 0x6b, 0x6b);
const COL_CHAR: egui::Color32 = egui::Color32::from_rgb(0x51, 0xcf, 0x66);
const COL_WARN: egui::Color32 = egui::Color32::from_rgb(0xff, 0xd4, 0x3b);
const COL_BORDER: egui::Color32 = egui::Color32::from_rgb(0x25, 0x25, 0x30);

// ---------------------------------------------------------------------------
// eframe::App implementation
// ---------------------------------------------------------------------------
impl eframe::App for C64App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // --- Timing / frame advancement ---
        if self.playing {
            let now = ctx.input(|i| i.time);
            if let Some(last) = self.last_time {
                let dt = (now - last) * self.speed as f64;
                self.accum += dt;
                let ft = 1.0 / data::FPS;
                while self.accum >= ft {
                    self.advance_frame();
                    self.accum -= ft;
                }
            }
            self.last_time = Some(now);
            ctx.request_repaint();
        } else {
            self.last_time = None;
        }

        // Request repaints while optimizer is running
        let opt_running = self.opt_progress.as_ref().map(|p| !p.lock().unwrap().done).unwrap_or(false);
        if opt_running {
            ctx.request_repaint();
        }

        // Advance sequence optimizer when current frame completes
        if self.seq_running && !opt_running {
            if let Some(ref progress) = self.opt_progress {
                let p = progress.lock().unwrap();
                if p.done {
                    let saved = p.baseline_score.saturating_sub(p.best_score);
                    self.seq_total_saved += saved;
                    self.seq_frames[self.seq_current_frame].baseline = p.baseline_score;
                    if let Some(ref state) = p.best_state {
                        self.seq_frames[self.seq_current_frame].result = Some(state.clone());
                    }
                    drop(p);
                    self.seq_current_frame += 1;
                    self.advance_sequence_optimizer();
                    ctx.request_repaint();
                }
            }
        }

        // --- Render the C64 frame ---
        let (pixels, stats, _sl_counts) = self.render_current_frame();

        // Build / update texture
        let color_image = egui::ColorImage::from_rgba_unmultiplied(
            [data::C64W, data::C64H],
            &pixels,
        );
        let tex_opts = egui::TextureOptions::NEAREST;
        match &mut self.texture {
            Some(tex) => tex.set(color_image, tex_opts),
            None => {
                self.texture = Some(ctx.load_texture("c64_screen", color_image, tex_opts));
            }
        }

        // --- Top panel: header ---
        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.colored_label(COL_ACCENT, egui::RichText::new("C64 CIRCLE FX").strong().size(13.0));
                ui.add_space(8.0);
                ui.colored_label(COL_DIM, egui::RichText::new("Generic Allocator").size(10.0));
                ui.add_space(8.0);
                ui.colored_label(COL_DIM, egui::RichText::new("EXTEND logo geosphere + E").size(10.0));
            });
        });

        // --- Right sidebar ---
        egui::SidePanel::right("sidebar")
            .default_width(280.0)
            .min_width(240.0)
            .show(ctx, |ui| {
                ui.style_mut().visuals.panel_fill = egui::Color32::from_rgb(0x0e, 0x0e, 0x14);
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.draw_sidebar(ui, &stats);
                });
            });

        // --- Central panel: main view ---
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.style_mut().visuals.panel_fill = egui::Color32::from_rgb(0x0c, 0x0c, 0x10);

            ui.vertical_centered(|ui| {
                // Main C64 display — hold mouse to compare with baseline
                if let Some(tex) = &self.texture {
                    let size = egui::vec2(
                        (data::C64W * data::SCALE) as f32,
                        (data::C64H * data::SCALE) as f32,
                    );
                    let image = egui::Image::new(tex)
                        .fit_to_exact_size(size)
                        .sense(egui::Sense::click());
                    let response = ui.add(image);
                    let was_comparing = self.opt_compare;
                    self.opt_compare = response.is_pointer_button_down_on();
                    if self.opt_compare != was_comparing {
                        ctx.request_repaint();
                    }
                }

                ui.add_space(8.0);

                // Controls bar
                self.draw_controls_bar(ui);

                ui.add_space(4.0);

                // Timeline bar
                self.draw_timeline(ui);

                // Phase label
                let phase = phase_label(self.frame);
                ui.colored_label(COL_DIM, egui::RichText::new(phase).size(10.0));

                ui.add_space(4.0);

                // Display options under viewport
                ui.horizontal_wrapped(|ui| {
                    ui.style_mut().spacing.button_padding = egui::vec2(6.0, 2.0);
                    option_toggle_compact(ui, "Grid", &mut self.opts.grid);
                    option_toggle_compact(ui, "Color", &mut self.opts.color);
                    option_toggle_compact(ui, "IDs", &mut self.opts.ids);
                    option_toggle_compact(ui, "Corrupt", &mut self.opts.corruption);
                    option_toggle_compact(ui, "C64", &mut self.opts.c64only);
                    option_toggle_compact(ui, "Mux", &mut self.opts.mux_overlay);
                    option_toggle_compact(ui, "Chars", &mut self.opts.show_chars);
                    option_toggle_compact(ui, "Sprites", &mut self.opts.show_sprites);
                    option_toggle_compact(ui, "Error", &mut self.opts.error_overlay);
                    option_toggle_compact(ui, "Ideal", &mut self.opts.ideal_render);
                });
            });
        });
    }
}

impl C64App {
    // -----------------------------------------------------------------------
    // Controls bar
    // -----------------------------------------------------------------------
    fn draw_controls_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.style_mut().spacing.button_padding = egui::vec2(8.0, 4.0);

            // Play / Pause
            let play_label = if self.playing { "Pause" } else { "Play" };
            if ui.button(play_label).clicked() {
                self.playing = !self.playing;
                if self.playing {
                    self.last_time = None;
                    self.accum = 0.0;
                }
            }

            // Step back/forward
            if ui.button("<").clicked() {
                self.playing = false;
                if self.frame == 0 {
                    self.frame = data::TOTAL_FRAMES - 1;
                } else {
                    self.frame -= 1;
                }
            }
            if ui.button(">").clicked() {
                self.playing = false;
                self.advance_frame();
            }

            // Reset
            if ui.button("Reset").clicked() {
                self.reset();
            }

            ui.add_space(6.0);

            // Speed slider
            ui.colored_label(COL_DIM, egui::RichText::new("Spd").size(11.0));
            let slider = egui::Slider::new(&mut self.speed, 0.1..=4.0)
                .step_by(0.1)
                .show_value(false)
                .custom_formatter(|v, _| format!("{:.1}x", v));
            ui.add(slider);
            ui.colored_label(COL_DIM, egui::RichText::new(format!("{:.1}x", self.speed)).size(11.0));

            ui.add_space(10.0);

            // Frame counter
            ui.colored_label(
                COL_ACCENT,
                egui::RichText::new(format!("Frame {} / {}", self.frame, data::TOTAL_FRAMES))
                    .size(12.0)
                    .strong(),
            );

            ui.add_space(10.0);

            // Corrupt counter
            ui.colored_label(
                COL_SPRITE,
                egui::RichText::new(format!("Corrupt: {}", self.corrupt_total))
                    .size(12.0)
                    .strong(),
            );
        });
    }

    // -----------------------------------------------------------------------
    // Timeline bar
    // -----------------------------------------------------------------------
    fn draw_timeline(&mut self, ui: &mut egui::Ui) {
        let desired_size = egui::vec2(
            (data::C64W * data::SCALE) as f32,
            32.0,
        );
        let (response, painter) = ui.allocate_painter(desired_size, egui::Sense::click_and_drag());
        let rect = response.rect;

        // Phase segments
        for &(s, e, color, label) in &PHASES {
            let x0 = rect.left() + (s as f32 / data::TOTAL_FRAMES as f32) * rect.width();
            let x1 = rect.left() + (e as f32 / data::TOTAL_FRAMES as f32) * rect.width();
            let phase_rect = egui::Rect::from_min_max(
                egui::pos2(x0, rect.top()),
                egui::pos2(x1, rect.bottom()),
            );
            painter.rect_filled(phase_rect, 0.0, color);
            painter.text(
                phase_rect.center(),
                egui::Align2::CENTER_CENTER,
                label,
                egui::FontId::monospace(8.0),
                COL_DIM,
            );
        }

        // Cursor
        let cursor_x = rect.left()
            + (self.frame as f32 / data::TOTAL_FRAMES as f32) * rect.width();
        let cursor_rect = egui::Rect::from_min_max(
            egui::pos2(cursor_x - 1.0, rect.top()),
            egui::pos2(cursor_x + 2.0, rect.bottom()),
        );
        painter.rect_filled(cursor_rect, 0.0, COL_ACCENT);

        // Border
        painter.rect_stroke(rect, 4.0, egui::Stroke::new(1.0, COL_BORDER), egui::StrokeKind::Outside);

        // Click or drag to seek/scan
        if response.clicked() || response.dragged() {
            if let Some(pos) = response.interact_pointer_pos() {
                let t = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
                let new_frame = ((t * data::TOTAL_FRAMES as f32) as usize).min(data::TOTAL_FRAMES - 1);
                if new_frame != self.frame {
                    self.frame = new_frame;
                    self.playing = false;
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Sidebar panels
    // -----------------------------------------------------------------------
    fn draw_sidebar(&mut self, ui: &mut egui::Ui, stats: &render::FrameStats) {
        // -- Legend --
        draw_panel(ui, "LEGEND", |ui| {
            ui.horizontal_wrapped(|ui| {
                legend_dot(ui, COL_CHAR, "Char");
                legend_dot(ui, COL_SPRITE, "Sprite");
                legend_dot(ui, egui::Color32::from_rgb(0x64, 0x20, 0x20), "No mux slot");
                legend_dot(ui, COL_WARN, "Corruption");
            });
        });

        // -- Frame Stats --
        draw_panel(ui, "FRAME STATS", |ui| {
            stat_row(ui, "Circles", &stats.total.to_string(), COL_TEXT);
            stat_row(ui, "Visible", &stats.visible.to_string(), COL_TEXT);
            stat_row(ui, "As sprites", &stats.sprites.to_string(), COL_SPRITE);
            stat_row(ui, "As chars", &stats.chars.to_string(), COL_CHAR);

            ui.separator();

            let conflict_color = if stats.conflicts > 0 { COL_WARN } else { COL_CHAR };
            stat_row(ui, "Char conflicts", &stats.conflicts.to_string(), conflict_color);

            let max_sl_color = if stats.max_sl as usize > data::MAX_SPR_LINE { COL_WARN } else { COL_SPRITE };
            stat_row(ui, "Max spr/scanline", &stats.max_sl.to_string(), max_sl_color);

            let mux_over_color = if stats.mux_overflows > 0 { COL_WARN } else { COL_CHAR };
            stat_row(ui, "Mux overflows", &stats.mux_overflows.to_string(), mux_over_color);

            stat_row(ui, "Mux slots used", &format!("{}/8", stats.mux_used), COL_SPRITE);

            stat_row(
                ui,
                "Frame discs",
                &format!("{} ({} raw)", stats.mem_discs, stats.on_screen_count),
                COL_TEXT,
            );
            stat_row(ui, "Frame bytes", &format!("{} B", stats.mem_bytes), COL_TEXT);

            let kb = self.accum_mem_bytes as f64 / 1024.0;
            stat_row(
                ui,
                "Total memory",
                &format!("{} B ({:.1} KB)", self.accum_mem_bytes, kb),
                COL_TEXT,
            );
        });

        // -- Optimizer --
        draw_panel(ui, "OPTIMIZER", |ui| {
            ui.horizontal(|ui| {
                ui.colored_label(COL_DIM, egui::RichText::new("Prune dist").size(10.0));
                ui.add(egui::Slider::new(&mut self.opts.prune_dist, 0.0..=8.0)
                    .step_by(0.5)
                    .custom_formatter(|v, _| {
                        if v == 0.0 { "off".to_string() } else { format!("{:.1}px", v) }
                    }));
            });
            ui.horizontal(|ui| {
                ui.colored_label(COL_DIM, egui::RichText::new("Iters/core").size(10.0));
                ui.add(egui::Slider::new(&mut self.opt_iterations, 1000..=50000)
                    .step_by(1000.0)
                    .show_value(true));
            });

            // Show baseline: from single-frame opt, sequence opt, or live
            let baseline = if self.opt_frame == Some(self.frame) {
                self.opt_progress.as_ref()
                    .map(|p| p.lock().unwrap().baseline_score)
                    .unwrap_or(stats.pixel_error)
            } else if self.seq_frames[self.frame].baseline > 0 {
                self.seq_frames[self.frame].baseline
            } else {
                stats.pixel_error
            };
            stat_row(ui, "Baseline error", &baseline.to_string(), COL_DIM);

            let is_running = self.opt_progress.as_ref()
                .map(|p| !p.lock().unwrap().done)
                .unwrap_or(false);

            if is_running {
                let p = self.opt_progress.as_ref().unwrap().lock().unwrap();
                let pct = if p.iterations_total > 0 {
                    (p.iterations_done as f32 / p.iterations_total as f32 * 100.0).min(100.0)
                } else { 0.0 };
                ui.colored_label(COL_ACCENT, egui::RichText::new(
                    format!("Running... {:.0}%", pct)).size(11.0));
                stat_row(ui, "Best error", &p.best_score.to_string(), COL_SPRITE);
            } else {
                let has_result = self.opt_frame == Some(self.frame)
                    && self.opt_progress.as_ref().map(|p| p.lock().unwrap().done).unwrap_or(false);

                ui.horizontal(|ui| {
                    if ui.button("Optimize frame").clicked() {
                        self.start_optimizer();
                    }
                    if has_result {
                        if ui.button("Reset").clicked() {
                            self.opt_progress = None;
                            self.opt_frame = None;
                        }
                    }
                });

                let has_result = self.opt_frame == Some(self.frame)
                    && self.opt_progress.as_ref().map(|p| p.lock().unwrap().done).unwrap_or(false);
                if has_result {
                    let p = self.opt_progress.as_ref().unwrap().lock().unwrap();
                    stat_row(ui, "Optimized error", &p.best_score.to_string(), COL_CHAR);
                    stat_row(ui, "Pixels saved",
                        &p.baseline_score.saturating_sub(p.best_score).to_string(), COL_ACCENT);
                    if p.sprites_demoted > 0 {
                        stat_row(ui, "Sprites→stamps", &p.sprites_demoted.to_string(), COL_DIM);
                    }
                    ui.colored_label(COL_DIM, egui::RichText::new(
                        "Hold mouse on viewport to compare").size(9.0));
                } else {
                    stat_row(ui, "Current error", &stats.pixel_error.to_string(), COL_DIM);
                }
            }

            ui.separator();

            // Sequence optimizer
            if self.seq_running {
                let pct = self.seq_current_frame as f32 / data::TOTAL_FRAMES as f32 * 100.0;
                ui.colored_label(COL_ACCENT, egui::RichText::new(
                    format!("Sequence: frame {}/{} ({:.0}%)", self.seq_current_frame, data::TOTAL_FRAMES, pct)
                ).size(11.0));
                stat_row(ui, "Total saved", &self.seq_total_saved.to_string(), COL_CHAR);
                if ui.button("Stop").clicked() {
                    self.seq_running = false;
                }
            } else {
                let seq_count = self.seq_frames.iter().filter(|f| f.result.is_some()).count();
                if seq_count > 0 {
                    stat_row(ui, "Optimized frames",
                        &format!("{}/{}", seq_count, data::TOTAL_FRAMES), COL_CHAR);
                    stat_row(ui, "Total saved", &self.seq_total_saved.to_string(), COL_ACCENT);
                }
                ui.horizontal(|ui| {
                    if ui.button("Optimize sequence").clicked() {
                        self.start_sequence_optimizer();
                    }
                    if seq_count > 0 {
                        if ui.button("Clear all").clicked() {
                            self.seq_frames = vec![SeqFrame::default(); data::TOTAL_FRAMES];
                            self.seq_total_saved = 0;
                        }
                    }
                });
            }
        });
    }

}

// ---------------------------------------------------------------------------
// Sidebar helper widgets
// ---------------------------------------------------------------------------
fn draw_panel(ui: &mut egui::Ui, title: &str, body: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::group(ui.style())
        .fill(COL_PANEL)
        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(0x1e, 0x1e, 0x2a)))
        .corner_radius(6.0)
        .show(ui, |ui| {
            // Header
            ui.colored_label(
                COL_DIM,
                egui::RichText::new(title).size(10.0).strong(),
            );
            ui.separator();
            // Body
            body(ui);
        });
    ui.add_space(6.0);
}

fn legend_dot(ui: &mut egui::Ui, color: egui::Color32, label: &str) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
        ui.painter().circle_filled(rect.center(), 4.0, color);
        ui.colored_label(COL_DIM, egui::RichText::new(label).size(10.0));
    });
}

fn stat_row(ui: &mut egui::Ui, label: &str, value: &str, value_color: egui::Color32) {
    ui.horizontal(|ui| {
        ui.colored_label(COL_DIM, egui::RichText::new(label).size(11.0));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.colored_label(value_color, egui::RichText::new(value).size(11.0).strong());
        });
    });
}

fn option_toggle_compact(ui: &mut egui::Ui, label: &str, value: &mut bool) {
    let text = egui::RichText::new(label).size(9.0);
    let btn = if *value {
        egui::Button::new(text.color(COL_ACCENT))
            .fill(egui::Color32::from_rgb(0x1a, 0x1a, 0x35))
            .stroke(egui::Stroke::new(1.0, COL_ACCENT))
    } else {
        egui::Button::new(text.color(COL_DIM))
            .fill(egui::Color32::from_rgb(0x15, 0x15, 0x20))
            .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(0x22, 0x23, 0x33)))
    };
    if ui.add(btn).clicked() {
        *value = !*value;
    }
}

fn phase_label(frame: usize) -> &'static str {
    for &(s, e, _, label) in &PHASES {
        if frame >= s && frame < e {
            return label;
        }
    }
    "--"
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------
fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1000.0, 700.0])
            .with_title("C64 Circle FX — Generic Allocator"),
        ..Default::default()
    };
    eframe::run_native(
        "C64 Circle FX",
        options,
        Box::new(|cc| Ok(Box::new(C64App::new(cc)))),
    )
}
