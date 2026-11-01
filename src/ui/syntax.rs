//! Display-only highlighting. Layout text is always identical to the supplied editor text.
use eframe::egui::{self, Color32, FontId, TextFormat, text::LayoutJob};
const TEXT: Color32 = Color32::from_rgb(220, 220, 220);
const KEY: Color32 = Color32::from_rgb(133, 185, 235);
const VALUE: Color32 = Color32::from_rgb(213, 176, 128);
const COMMENT: Color32 = Color32::from_rgb(132, 165, 123);
const PUNCT: Color32 = Color32::from_rgb(170, 170, 170);
#[derive(Default)]
struct UndoHistory {
    past: std::collections::VecDeque<String>,
    future: Vec<String>,
    last: String,
    changed_at: f64,
}
impl UndoHistory {
    fn record(&mut self, text: &str, now: f64, boundary: bool) {
        if self.last == text {
            return;
        }
        if boundary || self.past.is_empty() || now - self.changed_at > 0.4 {
            self.past.push_back(self.last.clone());
        }
        self.future.clear();
        self.last = text.into();
        self.changed_at = now;
        while self.past.len() > 64
            || self.past.iter().map(String::len).sum::<usize>() > 2 * 1024 * 1024
        {
            self.past.pop_front();
        }
    }
    fn undo(&mut self) -> Option<String> {
        let value = self.past.pop_back()?;
        self.future
            .push(std::mem::replace(&mut self.last, value.clone()));
        Some(value)
    }
    fn redo(&mut self) -> Option<String> {
        let value = self.future.pop()?;
        self.past
            .push_back(std::mem::replace(&mut self.last, value.clone()));
        Some(value)
    }
}
fn append(job: &mut LayoutJob, text: &str, color: Color32) {
    job.append(
        text,
        0.0,
        TextFormat {
            font_id: FontId::monospace(13.0),
            color,
            ..Default::default()
        },
    );
}
pub fn layout(text: &str) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = f32::INFINITY;
    let split = text
        .find("\r\n\r\n")
        .map(|i| i + 4)
        .or_else(|| text.find("\n\n").map(|i| i + 2));
    if !text.starts_with("HTTP/")
        && !text.lines().next().is_some_and(|line| {
            line.ends_with("HTTP/1.1")
                || line.ends_with("HTTP/1.0")
                || line.trim_end_matches('\r').ends_with("HTTP/1.1")
        })
    {
        body(&mut job, text);
        return job;
    }
    let (head, content) = split.map_or((text, ""), |i| text.split_at(i));
    for (index, line) in head.split_inclusive('\n').enumerate() {
        if index == 0 {
            if let Some(space) = line.find(' ') {
                append(&mut job, &line[..space], KEY);
                append(&mut job, &line[space..], VALUE);
            } else {
                append(&mut job, line, KEY);
            }
        } else if let Some(colon) = line.find(':') {
            append(&mut job, &line[..colon], KEY);
            append(&mut job, &line[colon..colon + 1], PUNCT);
            append(&mut job, &line[colon + 1..], TEXT);
        } else {
            append(&mut job, line, TEXT);
        }
    }
    body(&mut job, content);
    job
}
fn body(job: &mut LayoutJob, mut text: &str) {
    let mut in_tag = false;
    while !text.is_empty() {
        if text.starts_with("<!--") {
            let end = text.find("-->").map_or(text.len(), |i| i + 3);
            append(job, &text[..end], COMMENT);
            text = &text[end..];
            continue;
        }
        let first = text.chars().next().unwrap();
        if first == '<' {
            in_tag = true;
            append(job, "<", PUNCT);
            text = &text[1..];
            continue;
        }
        if first == '>' {
            in_tag = false;
            append(job, ">", PUNCT);
            text = &text[1..];
            continue;
        }
        if (in_tag || first == '"') && (first == '\'' || first == '"') {
            let tail = &text[1..];
            let mut escaped = false;
            let mut end = text.len();
            for (i, ch) in tail.char_indices() {
                if ch == first && !escaped {
                    end = i + 2;
                    break;
                }
                escaped = ch == '\\' && !escaped;
            }
            append(job, &text[..end], VALUE);
            text = &text[end..];
            continue;
        }
        let end = text
            .char_indices()
            .skip(1)
            .find(|(_, ch)| matches!(ch, '<' | '>' | '"') || (in_tag && *ch == '\''))
            .map_or(text.len(), |(i, _)| i);
        append(job, &text[..end], if in_tag { KEY } else { TEXT });
        text = &text[end..];
    }
}
pub fn editor(
    ui: &mut egui::Ui,
    id: egui::Id,
    text: &mut String,
    editable: bool,
    rows: usize,
) -> egui::Response {
    // egui's built-in undo compares cursor positions as well as text. Keeping text-only
    // revisions preserves redo when the operator clicks or sends between undo and redo.
    let history = editable.then(|| {
        ui.ctx().data_mut(|data| {
            data.get_temp_mut_or_insert_with(id.with("undo-text"), || {
                std::sync::Arc::new(std::sync::Mutex::new(UndoHistory {
                    last: text.clone(),
                    ..Default::default()
                }))
            })
            .clone()
        })
    });
    let now = ui.input(|input| input.time);
    if let Some(history) = &history {
        let mut history = history.lock().unwrap_or_else(|e| e.into_inner());
        history.record(text, now, true);
        if ui.memory(|memory| memory.has_focus(id)) {
            let undo = ui.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::Z));
            let redo = ui.input_mut(|input| {
                input.consume_key(egui::Modifiers::CTRL, egui::Key::Y)
                    || input
                        .consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::Z)
            });
            let revision = if undo {
                history.undo()
            } else if redo {
                history.redo()
            } else {
                None
            };
            if let Some(revision) = revision {
                *text = revision;
                if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), id) {
                    state
                        .cursor
                        .set_char_range(Some(egui::text::CCursorRange::one(
                            egui::text::CCursor::new(text.chars().count()),
                        )));
                    state.store(ui.ctx(), id);
                }
            }
        }
    }
    let before = text.clone();
    let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, _width: f32| {
        let mut job = layout(buf.as_str());
        let font = egui::TextStyle::Monospace.resolve(ui.style());
        for section in &mut job.sections {
            section.format.font_id = font.clone();
        }
        ui.fonts_mut(|fonts| fonts.layout_job(job))
    };
    let response = ui
        .horizontal_top(|ui| {
            let count = text.lines().count().clamp(1, 5000);
            let numbers = (1..=count)
                .map(|n| n.to_string())
                .collect::<Vec<_>>()
                .join("\n");
            ui.label(egui::RichText::new(numbers).monospace().color(PUNCT));
            if editable {
                ui.add(
                    egui::TextEdit::multiline(text)
                        .id(id)
                        .code_editor()
                        .frame(egui::Frame::NONE)
                        .layouter(&mut layouter)
                        .char_limit(128 * 1024)
                        .desired_width(f32::INFINITY)
                        .desired_rows(rows),
                )
            } else {
                ui.add(
                    egui::TextEdit::multiline(&mut text.as_str())
                        .id(id)
                        .code_editor()
                        .frame(egui::Frame::NONE)
                        .layouter(&mut layouter)
                        .desired_width(f32::INFINITY)
                        .desired_rows(rows),
                )
            }
        })
        .inner;
    if editable && before != *text {
        let boundary = ui.input(|input| {
            input
                .events
                .iter()
                .any(|event| matches!(event, egui::Event::Paste(_)))
        });
        history
            .as_ref()
            .unwrap()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .record(text, now, boundary);
    }
    if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), id) {
        state.clear_undoer();
        state.store(ui.ctx(), id);
    }
    response
}
pub fn forget_editor(ctx: &egui::Context, id: egui::Id) {
    ctx.data_mut(|data| {
        data.remove::<std::sync::Arc<std::sync::Mutex<UndoHistory>>>(id.with("undo-text"))
    });
    if let Some(mut state) = egui::TextEdit::load_state(ctx, id) {
        state.clear_undoer();
        state.store(ctx, id);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn highlighting_preserves_http_html_and_unicode_exactly() {
        let text =
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n<!-- hi --><a href=\"/ü\">link</a>";
        let job = layout(text);
        assert_eq!(job.text, text);
        assert!(job.sections.len() > 8);
        assert!(job.sections.iter().any(|s| s.format.color == COMMENT));
        assert!(job.sections.iter().any(|s| s.format.color == VALUE));
    }
}

#[cfg(test)]
mod undo_tests {
    use super::*;
    fn frame(context: &egui::Context, text: &mut String, time: f64, events: Vec<egui::Event>) {
        let mut output = context.run_ui(
            egui::RawInput {
                time: Some(time),
                events,
                ..Default::default()
            },
            |ui| {
                let response = editor(ui, egui::Id::new("undo-test"), text, true, 5);
                if time == 0.0 {
                    response.request_focus();
                }
            },
        );
        output.textures_delta.clear();
    }
    fn key(key: egui::Key) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers {
                ctrl: true,
                command: true,
                ..Default::default()
            },
        }
    }
    #[test]
    fn ctrl_z_and_ctrl_y_restore_editor_text() {
        let context = egui::Context::default();
        let mut text = "GET / HTTP/1.1\n\n".to_owned();
        let original = text.clone();
        frame(&context, &mut text, 0.0, vec![]);
        frame(
            &context,
            &mut text,
            1.0,
            vec![egui::Event::Text("edited".into())],
        );
        let edited = text.clone();
        assert_ne!(edited, original);
        frame(&context, &mut text, 2.0, vec![key(egui::Key::Z)]);
        assert_eq!(text, original);
        frame(&context, &mut text, 3.0, vec![key(egui::Key::Y)]);
        assert_eq!(text, edited);
    }
}

pub fn find_next(
    ui: &mut egui::Ui,
    id: egui::Id,
    text: &str,
    query: &str,
    next: &mut usize,
) -> Option<usize> {
    if query.is_empty() {
        return None;
    }
    let start = (*next).min(text.len());
    let start = text.floor_char_boundary(start);
    let at = text[start..]
        .find(query)
        .map(|n| start + n)
        .or_else(|| text.find(query))?;
    *next = at + query.len();
    let first = text[..at].chars().count();
    let end = first + query.chars().count();
    let mut state = egui::TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
    state
        .cursor
        .set_char_range(Some(egui::text::CCursorRange::two(
            egui::text::CCursor::new(first),
            egui::text::CCursor::new(end),
        )));
    state.store(ui.ctx(), id);
    ui.memory_mut(|memory| memory.request_focus(id));
    Some(text[..at].bytes().filter(|b| *b == b'\n').count())
}
