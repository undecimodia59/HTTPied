//! CPU layout measurement only. No window, GPU presentation or input latency claims.
use eframe::egui;
fn main() {
    let context = egui::Context::default();
    let mut draft = "GET / HTTP/1.1\r\nHost: localhost\r\n\r\n".repeat(1024);
    let mut samples = Vec::new();
    let mut max_rows = 0;
    for frame in 0..330 {
        let mut rows_drawn = 0;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1024.0, 768.0),
            )),
            ..Default::default()
        };
        let start = std::time::Instant::now();
        let _output = context.run_ui(input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(300.0)
                    .vertical_scroll_offset((frame * 7000 % 2_000_000) as f32)
                    .show_rows(ui, 22.0, 100_000, |ui, range| {
                        rows_drawn = range.len();
                        for row in range {
                            ui.label(format!(
                                "{row:06}    GET    http://fixture.local/item/{row}    200"
                            ));
                        }
                    });
                ui.add(
                    egui::TextEdit::multiline(&mut draft)
                        .code_editor()
                        .char_limit(65_536)
                        .desired_rows(12),
                );
            });
        });
        max_rows = max_rows.max(rows_drawn);
        if frame >= 30 {
            samples.push(start.elapsed().as_secs_f64() * 1000.0);
        }
    }
    samples.sort_by(f64::total_cmp);
    assert!(
        max_rows < 100,
        "virtualization unexpectedly rendered {max_rows} rows"
    );
    println!(
        "{{\"mode\":\"release CPU egui layout only; no window/input/presentation\",\"logical_rows\":100000,\"max_rows_drawn_per_layout\":{max_rows},\"editor_bytes\":{},\"warmup_frames\":30,\"samples\":300,\"median_ms\":{},\"p95_ms\":{}}}",
        draft.len(),
        samples[150],
        samples[285]
    );
}
