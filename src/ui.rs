//! Native project workspace. Rendering, storage and network work use separate bounded queues.
use crate::{
    model::Event,
    proxy::{self, Config},
};
use eframe::egui;
mod coder;
mod compare;
mod features;
mod history;
mod log_view;
mod results;
mod settings;
mod syntax;
mod widgets;
mod workflow;
use std::collections::VecDeque;
use tokio::sync::{mpsc, watch};
use widgets::*;
use workflow::{Tab, Workflow};

const PREVIEW: usize = 64 * 1024;
const RETAINED_BYTES: usize = 16 * 1024 * 1024;
const RETAINED_ROWS: usize = 128;

pub fn run(
    mut config: Config,
    tx: mpsc::Sender<Event>,
    rx: mpsc::Receiver<Event>,
    stop: watch::Sender<bool>,
    stop_rx: watch::Receiver<bool>,
) -> Result<(), Box<dyn std::error::Error>> {
    let configured = config.listen;
    let (startup_tx, startup_rx) = tokio::sync::oneshot::channel::<(
        crate::state::Settings,
        std::sync::Arc<crate::authority::Authority>,
    )>();
    let (control, pending_rx) =
        crate::control::Interception::channel(16, std::time::Duration::from_secs(60));
    config.interception = Some(control.clone());
    let (policy_tx, policy_rx) =
        watch::channel(std::sync::Arc::new(crate::policy::Policy::default()));
    config.policy = Some(policy_rx);
    let (job_tx, job_rx) = mpsc::channel(2);
    let (result_tx, result_rx) = mpsc::channel(2);
    let mut browser = crate::browser::Service::start();
    let browser_tx = browser.commands.clone();
    let browser_rx = browser.messages.take().unwrap();
    let thread = std::thread::spawn(move || -> std::io::Result<()> {
        let runtime = match tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(error) => {
                let _ = tx.blocking_send(Event::Error(format!("runtime: {error}")));
                return Err(error);
            }
        };
        runtime.block_on(async move {
            let mut shutdown = stop_rx.clone();
            let settings = tokio::select! { _ = shutdown.changed() => return Ok(()), settings = startup_rx => match settings {Ok(value)=>value, Err(_)=>return Ok(())}};
            let (settings,ca)=settings;
            config.ca=Some(ca);
            config.listen = if configured!=Config::default().listen {configured}else{settings.listen.parse().map_err(|e| std::io::Error::other(format!("{e}")))?};
            config.max_capture = settings.capture_mib * 1024 * 1024;
            config.total_timeout = std::time::Duration::from_secs(settings.http_timeout);
            config.idle_timeout = std::time::Duration::from_secs(settings.idle_timeout);
            config.connect_timeout = std::time::Duration::from_secs(settings.connect_timeout);
            let listener = match proxy::bind(&config).await {
                Ok(listener) => listener,
                Err(error) => {
                    let _ = tx.send(Event::Error(format!("listener: {error}"))).await;
                    return Err(error);
                }
            };
            let address = listener.local_addr()?;
            eprintln!("READY {address}");
            let _ = tx.send(Event::Listening(address)).await;
            let error_tx = tx.clone();
            let jobs = tokio::spawn(crate::jobs::run(
                job_rx,
                result_tx,
                config.clone(),
                stop_rx.clone(),
            ));
            let result = proxy::serve(listener, config, tx, stop_rx).await;
            jobs.abort();
            let _ = jobs.await;
            if let Err(error) = &result {
                let _ = error_tx
                    .send(Event::Error(format!("network: {error}")))
                    .await;
            }
            result
        })
    });
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1240.0, 860.0])
            .with_min_inner_size([940.0, 700.0]),
        ..Default::default()
    };
    let result = eframe::run_native(
        "HTTPied — Intercept · History · Replay",
        options,
        Box::new(move |cc| {
            theme(&cc.egui_ctx);
            let mut app = Spike::new(rx, configured);
            app.workflow = Workflow::new(control, pending_rx, job_tx, result_rx, configured);
            app.workflow.set_browser(browser_tx, browser_rx);
            app.features.startup = Some(startup_tx);
            app.features.policy = Some(policy_tx);
            Ok(Box::new(app))
        }),
    );
    let _ = stop.send(true);
    browser.shutdown();
    thread.join().map_err(|_| "network thread panicked")??;
    result.map_err(|e| e.to_string().into())
}

struct Entry {
    id: u64,
    event: Event,
    size: usize,
    loaded: bool,
}
struct History {
    entries: VecDeque<Entry>,
    bytes: usize,
    evicted: u64,
    next_id: u64,
    byte_limit: usize,
    row_limit: usize,
}
impl History {
    fn new(byte_limit: usize, row_limit: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            bytes: 0,
            evicted: 0,
            next_id: 1,
            byte_limit,
            row_limit,
        }
    }
    fn push(&mut self, event: Event) {
        let size = event_size(&event);
        let id = self.next_id;
        self.next_id += 1;
        // Even an oversized single event cannot bypass retention bounds.
        if size > self.byte_limit || self.row_limit == 0 {
            self.evicted += 1;
            return;
        }
        while self.entries.len() >= self.row_limit || self.bytes + size > self.byte_limit {
            if let Some(entry) = self.entries.pop_front() {
                self.bytes -= entry.size;
                self.evicted += 1;
            } else {
                break;
            }
        }
        self.bytes += size;
        self.entries.push_back(Entry {
            id,
            event,
            size,
            loaded: true,
        });
    }
}
fn event_size(event: &Event) -> usize {
    match event {
        Event::Exchange(e) => {
            e.original.0.len()
                + e.outgoing.len()
                + e.response.len()
                + e.response_outgoing.len()
                + e.parsed.method.len()
                + e.parsed.target.len()
                + e.parsed
                    .headers
                    .iter()
                    .map(|(k, v)| k.len() + v.len())
                    .sum::<usize>()
        }
        Event::WebSocket {
            payload, target, ..
        } => payload.len() + target.len() + 128,
        Event::Error(e) => e.len(),
        Event::Tunnel { authority, .. } | Event::InterceptedTls { authority, .. } => {
            authority.len()
        }
        Event::Listening(_) => 0,
    }
}
fn event_label(event: &Event) -> String {
    match event {
        Event::Exchange(e) => {
            let status = e.response.split(|b| *b == b'\n').next().unwrap_or_default();
            format!(
                "{}  {}  —  {}",
                e.parsed.method,
                preview(e.parsed.target.as_bytes(), 512),
                preview(status, 128).trim()
            )
        }
        Event::WebSocket {
            target,
            direction,
            kind,
            ..
        } => format!("WS {target} {direction} {kind}"),
        Event::Error(e) => format!("Error — {}", preview(e.as_bytes(), 512)),
        Event::Tunnel {
            authority,
            uploaded,
            downloaded,
        } => format!("CONNECT {authority} — opaque tunnel ↑{uploaded} ↓{downloaded} B"),
        Event::InterceptedTls {
            authority,
            uploaded,
            downloaded,
        } => format!(
            "TLS {authority} — decrypted stream ↑{uploaded} ↓{downloaded} B (no HTTP capture)"
        ),
        Event::Listening(address) => format!("Listening on {address}"),
    }
}
fn preview(bytes: &[u8], limit: usize) -> String {
    let end = bytes.len().min(limit);
    let mut text = String::from_utf8_lossy(&bytes[..end]).into_owned();
    if text.len() > limit {
        let mut boundary = limit;
        while !text.is_char_boundary(boundary) {
            boundary -= 1;
        }
        text.truncate(boundary);
    }
    text
}
fn hex_preview(bytes: &[u8]) -> String {
    let mut result = String::new();
    use std::fmt::Write;
    // Cap expanded text too: 512 rows / 8 KiB of source bytes.
    for (row, chunk) in bytes.chunks(16).take(512).enumerate() {
        let _ = write!(result, "{:08x}  ", row * 16);
        for index in 0..16 {
            if let Some(byte) = chunk.get(index) {
                let _ = write!(result, "{byte:02x} ");
            } else {
                result.push_str("   ");
            }
        }
        result.push_str(" | ");
        for byte in chunk {
            result.push(if byte.is_ascii_graphic() || *byte == b' ' {
                *byte as char
            } else {
                '.'
            });
        }
        result.push('\n');
    }
    if bytes.len() > 8192 {
        result.push_str("… Hex preview truncated after 8 KiB of source bytes.\n");
    }
    result
}
fn row_fields(event: &Event) -> (String, String, String, String) {
    match event {
        Event::Exchange(e) => {
            let status = e.response_status.to_string();
            (
                preview(e.parsed.method.as_bytes(), 24),
                status,
                preview(e.parsed.target.as_bytes(), 512),
                format!(
                    "{} B{}",
                    e.response_total_bytes,
                    if e.response_truncated { " *" } else { "" }
                ),
            )
        }
        Event::WebSocket {
            target,
            kind,
            total,
            ..
        } => (
            "WS".into(),
            kind.clone(),
            target.clone(),
            crate::state::size(*total as u64, false),
        ),
        Event::Error(error) => (
            "ERROR".into(),
            "!".into(),
            preview(error.as_bytes(), 512),
            "—".into(),
        ),
        Event::Tunnel { authority, .. } => (
            "CONNECT".into(),
            "TCP".into(),
            authority.clone(),
            "—".into(),
        ),
        Event::InterceptedTls { authority, .. } => (
            "CONNECT".into(),
            "TLS".into(),
            authority.clone(),
            "—".into(),
        ),
        Event::Listening(address) => ("LISTEN".into(), "—".into(), address.to_string(), "—".into()),
    }
}
fn column_layout(
    columns: &[String],
    widths: &std::collections::BTreeMap<String, f32>,
    width: f32,
) -> Vec<(SortColumn, f32, f32, String)> {
    let mut start = 0.0;
    let flexible = |name: &str| {
        matches!(name, "Destination" | "Host" | "Endpoint") && !widths.contains_key(name)
    };
    let fixed = columns
        .iter()
        .filter(|c| !flexible(c))
        .map(|c| {
            widths.get(c).copied().unwrap_or(match c.as_str() {
                "#" => 58.0,
                "Method" => 80.0,
                "MIME" => 170.0,
                _ => 90.0,
            })
        })
        .sum::<f32>();
    let flexible_count = columns.iter().filter(|name| flexible(name)).count().max(1) as f32;
    columns
        .iter()
        .filter_map(|name| {
            let column = match name.as_str() {
                "#" => SortColumn::Id,
                "Method" => SortColumn::Method,
                "Status" => SortColumn::Status,
                "Destination" => SortColumn::Destination,
                "Host" => SortColumn::Host,
                "Endpoint" => SortColumn::Endpoint,
                "Duration" => SortColumn::Duration,
                "TTFB" => SortColumn::Ttfb,
                "Held" => SortColumn::Held,
                "Size" => SortColumn::Size,
                "MIME" => SortColumn::Mime,
                "Protocol" => SortColumn::Protocol,
                "Stream ID" => SortColumn::Stream,
                _ => return None,
            };
            let span = widths
                .get(name)
                .copied()
                .unwrap_or(match name.as_str() {
                    "Destination" | "Host" | "Endpoint" => {
                        ((width - fixed) / flexible_count).max(160.0)
                    }
                    "#" => 58.0,
                    "Method" => 80.0,
                    "MIME" => 170.0,
                    _ => 90.0,
                })
                .clamp(40.0, 2000.0);
            let result = (column, start, span, name.clone());
            start += span;
            Some(result)
        })
        .collect()
}
fn traffic_row(
    ui: &mut egui::Ui,
    entry: &Entry,
    layout: &[(SortColumn, f32, f32, String)],
    binary: bool,
    selected: bool,
) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 28.0), egui::Sense::click());
    if selected || response.hovered() {
        ui.painter().rect_filled(
            rect,
            0,
            if selected {
                egui::Color32::from_rgb(42, 55, 73)
            } else {
                egui::Color32::from_rgb(36, 38, 42)
            },
        );
    }
    let (method, status, destination, _) = row_fields(&entry.event);
    let (host, endpoint) = event_destination_parts(&entry.event);
    for (column, start, span, _) in layout {
        let value = match *column {
            SortColumn::Id => entry.id.to_string(),
            SortColumn::Method => method.clone(),
            SortColumn::Status => status.clone(),
            SortColumn::Destination => destination.clone(),
            SortColumn::Host => host.clone(),
            SortColumn::Endpoint => endpoint.clone(),
            _ => {
                if let Event::Exchange(e) = &entry.event {
                    match *column {
                        SortColumn::Duration => format!("{:.1} ms", e.elapsed_ms),
                        SortColumn::Ttfb => format!("{:.1} ms", e.first_byte_ms),
                        SortColumn::Held => format!("{:.1} ms", e.held_ms),
                        SortColumn::Size => format!(
                            "{}{}",
                            crate::state::size(e.response_total_bytes, binary),
                            if e.response_truncated { " *" } else { "" }
                        ),
                        SortColumn::Mime => e.response_mime.clone(),
                        SortColumn::Protocol => {
                            crate::state::protocol_parts(&e.protocol).0.to_owned()
                        }
                        SortColumn::Stream => crate::state::protocol_parts(&e.protocol)
                            .1
                            .map(|id| id.to_string())
                            .unwrap_or_else(|| "—".into()),
                        _ => String::new(),
                    }
                } else {
                    "—".into()
                }
            }
        };
        let cell = egui::Rect::from_min_size(
            egui::pos2(rect.left() + start, rect.top()),
            egui::vec2(*span, rect.height()),
        )
        .intersect(rect);
        ui.painter().with_clip_rect(cell).text(
            egui::pos2(cell.left() + 8.0, cell.center().y),
            egui::Align2::LEFT_CENTER,
            value,
            egui::FontId::monospace(12.0),
            if *column == SortColumn::Method {
                ACCENT
            } else {
                ui.visuals().text_color()
            },
        );
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            true,
            selected,
            format!("Request {} {method} {status} {destination}", entry.id),
        )
    });
    response.on_hover_text(destination)
}
fn event_destination_parts(event: &Event) -> (String, String) {
    match event {
        Event::Exchange(e) => crate::state::destination_parts(&e.parsed.target),
        Event::WebSocket { target, .. } => crate::state::destination_parts(target),
        Event::Tunnel { authority, .. } | Event::InterceptedTls { authority, .. } => {
            crate::state::destination_parts(authority)
        }
        _ => Default::default(),
    }
}
#[derive(Clone, Copy, PartialEq)]
enum Pane {
    Original,
    Outgoing,
    Response,
    SentResponse,
    Draft,
}
#[derive(Clone, Copy, PartialEq)]
enum SortColumn {
    Id,
    Method,
    Status,
    Destination,
    Host,
    Endpoint,
    Size,
    Duration,
    Protocol,
    Stream,
    Ttfb,
    Held,
    Mime,
}
struct Spike {
    coder: coder::Coder,
    compare_view: compare::CompareView,
    inspect: history::Inspector,
    workflow: Workflow,
    features: features::Features,
    rx: mpsc::Receiver<Event>,
    history: History,
    selected: Option<u64>,
    pane: Pane,
    draft: String,
    text: String,
    detail: String,
    status: String,
    sort_column: SortColumn,
    sort_descending: bool,
    disconnected: bool,
    filter: String,
    logs: crate::activity::Book,
    log_view: log_view::LogView,
    hex: bool,
    decoded_notice: bool,
    preview_note: Option<String>,
    confirm_clear: bool,
    toast: Option<(String, std::time::Instant)>,
    scroll_selection: bool,
    reset_draft_undo: bool,
}
impl Spike {
    fn new(rx: mpsc::Receiver<Event>, listen: std::net::SocketAddr) -> Self {
        Self {
            rx,
            coder: Default::default(),
            compare_view: Default::default(),
            inspect: Default::default(),
            features: features::Features::new(),
            workflow: Workflow::offline(listen),
            history: History::new(RETAINED_BYTES, RETAINED_ROWS),
            selected: None,
            pane: Pane::Original,
            draft: String::new(),
            text: String::new(),
            detail: "Select a captured exchange to inspect it.".into(),
            status: format!("Starting proxy on {listen}…"),
            sort_column: SortColumn::Id,
            sort_descending: false,
            disconnected: false,
            filter: String::new(),
            logs: Default::default(),
            log_view: Default::default(),
            hex: false,
            decoded_notice: false,
            preview_note: None,
            confirm_clear: false,
            toast: None,
            scroll_selection: false,
            reset_draft_undo: false,
        }
    }
    fn receive(&mut self, event: Event) {
        if let Event::Error(message) = event {
            self.add_log(crate::activity::Level::Error, &message);
            return;
        }
        if let Event::Listening(address) = event {
            self.workflow.listen = address;
            self.workflow.listener_ready = true;
            self.status = format!("Listening on {address}");
            return;
        }
        if self.features.open {
            self.project_command(crate::project::Command::Store {
                id: self.history.next_id,
                event: event.clone(),
            });
            self.features.total += 1;
        }
        let auto_select = self.selected.is_none();
        self.history.push(event);
        if let Some(id) = self.selected
            && !self.history.entries.iter().any(|e| e.id == id)
        {
            self.selected = None;
            self.text.clear();
            self.draft.clear();
            self.detail = "Selected capture was evicted by the retention limit.".into();
        }
        if auto_select && let Some(entry) = self.history.entries.back() {
            self.select(entry.id);
        }
    }
    fn select(&mut self, id: u64) {
        if self.selected != Some(id) {
            self.reset_draft_undo = true;
        }
        if let Some(entry) = self.history.entries.iter().find(|entry| entry.id == id) {
            if let Event::Exchange(e) = &entry.event {
                if e.protocol.starts_with("HTTP/2") && self.pane == Pane::Original {
                    self.pane = Pane::Outgoing;
                }
            } else {
                self.pane = Pane::Original;
            }
        }
        self.selected = Some(id);
        if self.history.entries.iter().any(|e| e.id == id && !e.loaded) {
            self.project_command(crate::project::Command::Detail(id));
            self.text.clear();
            self.detail = "Loading capture…".into();
            return;
        }
        self.draft = match self
            .history
            .entries
            .iter()
            .find(|e| e.id == id)
            .map(|e| &e.event)
        {
            Some(Event::Exchange(e)) => preview(&e.original.0, PREVIEW),
            _ => String::new(),
        };
        self.refresh();
    }
    fn refresh(&mut self) {
        self.decoded_notice = false;
        self.preview_note = None;
        let Some(entry) = self
            .history
            .entries
            .iter()
            .find(|e| Some(e.id) == self.selected)
        else {
            return;
        };
        match &entry.event {
            Event::Exchange(e) => {
                let bytes = match self.pane {
                    Pane::Original | Pane::Draft => &e.original.0,
                    Pane::Outgoing => &e.outgoing,
                    Pane::Response => &e.response,
                    Pane::SentResponse => {
                        if e.response_outgoing.is_empty() {
                            &e.response
                        } else {
                            &e.response_outgoing
                        }
                    }
                };
                self.text = if self.features.parameters {
                    crate::exchange::parameters(bytes, &e.parsed.target)
                } else if self.features.decoded {
                    crate::exchange::decoded(bytes)
                        .unwrap_or_else(|error| format!("Decoded view unavailable: {error}"))
                } else if self.hex {
                    hex_preview(bytes)
                } else if self.features.auto_decode || self.features.pretty {
                    let view = crate::exchange::auto_preview(bytes);
                    self.decoded_notice = view.decoded;
                    self.preview_note = view.note;
                    view.text
                } else {
                    preview(bytes, PREVIEW)
                };
                if self.pane == Pane::Original
                    && e.original.0.is_empty()
                    && e.protocol.starts_with("HTTP/2")
                {
                    self.text="Original HTTP/2 frames were not retained. Outgoing shows the normalized request.".into();
                }
                if self.features.pretty {
                    self.text = crate::exchange::pretty(&self.text);
                }
                let transformations = e.transformations.join("; ");
                let capture_note = if self.pane == Pane::Response && e.response_truncated {
                    format!(
                        "; capture contains {} of {} bytes",
                        e.response.len(),
                        e.response_total_bytes
                    )
                } else {
                    String::new()
                };
                self.detail = format!(
                    "{} bytes • {}{} • {}{}",
                    bytes.len(),
                    if self.hex {
                        "Hex preview (8 KiB source cap)"
                    } else {
                        "UTF-8 lossy preview (64 KiB cap)"
                    },
                    if bytes.len() > if self.hex { 8192 } else { PREVIEW } {
                        " • truncated"
                    } else {
                        ""
                    },
                    transformations,
                    capture_note
                );
            }
            Event::WebSocket {
                connection,
                direction,
                kind,
                payload,
                total,
                truncated,
                closed,
                ..
            } => {
                self.text = if self.hex {
                    hex_preview(payload)
                } else {
                    preview(payload, PREVIEW)
                };
                self.detail = format!(
                    "Connection {connection} · {direction} · {kind} · {total} bytes · reassembled message; original frames not retained{}{}",
                    if *truncated {
                        " · preview truncated"
                    } else {
                        ""
                    },
                    if *closed { " · closed" } else { "" }
                );
            }
            event => {
                self.text = event_label(event);
                self.detail =
                    "Transport/error metadata only; no HTTP message bytes captured.".into();
            }
        }
    }
    fn visible_ids(&self) -> Vec<u64> {
        let mut filter = self.features.state.filter.clone();
        filter.text = self.filter.clone();
        let mut entries: Vec<_> = self
            .history
            .entries
            .iter()
            .filter(|e| filter.matches(&e.event, &self.features.state.scope))
            .collect();
        entries.sort_by(|a, b| {
            let fields = |e: &Entry| row_fields(&e.event);
            let status = |e: &Entry| {
                if let Event::Exchange(x) = &e.event {
                    x.response_status
                } else {
                    0
                }
            };
            let size = |e: &Entry| {
                if let Event::Exchange(x) = &e.event {
                    x.response_total_bytes
                } else {
                    0
                }
            };
            let order = match self.sort_column {
                SortColumn::Id => a.id.cmp(&b.id),
                SortColumn::Method => fields(a).0.cmp(&fields(b).0),
                SortColumn::Status => status(a).cmp(&status(b)),
                SortColumn::Destination => {
                    fields(a).2.to_lowercase().cmp(&fields(b).2.to_lowercase())
                }
                SortColumn::Host => event_destination_parts(&a.event)
                    .0
                    .cmp(&event_destination_parts(&b.event).0),
                SortColumn::Endpoint => event_destination_parts(&a.event)
                    .1
                    .cmp(&event_destination_parts(&b.event).1),
                SortColumn::Size => size(a).cmp(&size(b)),
                SortColumn::Duration | SortColumn::Ttfb | SortColumn::Held => {
                    let time = |entry: &Entry| {
                        if let Event::Exchange(e) = &entry.event {
                            match self.sort_column {
                                SortColumn::Duration => e.elapsed_ms,
                                SortColumn::Ttfb => e.first_byte_ms,
                                _ => e.held_ms,
                            }
                        } else {
                            0.0
                        }
                    };
                    time(a).total_cmp(&time(b))
                }
                SortColumn::Protocol => {
                    let protocol = |entry: &Entry| {
                        if let Event::Exchange(e) = &entry.event {
                            crate::state::protocol_parts(&e.protocol).0.to_owned()
                        } else {
                            String::new()
                        }
                    };
                    protocol(a).cmp(&protocol(b))
                }
                SortColumn::Stream => {
                    let stream = |entry: &Entry| {
                        if let Event::Exchange(e) = &entry.event {
                            crate::state::protocol_parts(&e.protocol).1.unwrap_or(0)
                        } else {
                            0
                        }
                    };
                    stream(a).cmp(&stream(b))
                }
                SortColumn::Mime => {
                    let mime = |entry: &Entry| {
                        if let Event::Exchange(e) = &entry.event {
                            e.response_mime.clone()
                        } else {
                            String::new()
                        }
                    };
                    mime(a).cmp(&mime(b))
                }
            }
            .then_with(|| a.id.cmp(&b.id));
            if self.sort_descending {
                order.reverse()
            } else {
                order
            }
        });
        entries.into_iter().map(|e| e.id).collect()
    }
    fn move_selection(&mut self, direction: isize) {
        let ids = self.visible_ids();
        if ids.is_empty() {
            return;
        }
        let next = ids
            .iter()
            .position(|id| Some(*id) == self.selected)
            .map(|index| index.saturating_add_signed(direction).min(ids.len() - 1))
            .unwrap_or(0);
        self.select(ids[next]);
        self.scroll_selection = true;
    }

    fn clear(&mut self) {
        self.history.entries.clear();
        self.history.bytes = 0;
        self.selected = None;
        self.text.clear();
        self.draft.clear();
        self.preview_note = None;
        self.decoded_notice = false;
        self.reset_draft_undo = true;
        self.confirm_clear = false;
        self.features.selection.clear();
        self.features.pending_transfer = None;
        self.detail = "View cleared; saved traffic remains in the project.".into();
        self.toast = Some(("Session captures cleared".into(), std::time::Instant::now()));
    }
    fn content(&mut self, ui: &mut egui::Ui) {
        if self.features.open {
            self.keyboard_actions(ui);
            self.workflow_content(ui);
        } else {
            self.start_screen(ui);
        }
    }
    fn workspace(&mut self, ui: &mut egui::Ui) {
        self.history_filters(ui);
        if self.disconnected {
            ui.colored_label(
                ERROR,
                "Proxy stopped. Restart HTTPied; see the terminal for details.",
            );
        }
        if self.confirm_clear {
            ui.horizontal(|ui| {
                ui.label("Clear the current view? Saved captures remain in the project.");
                if ui.button("Clear").clicked() {
                    self.clear();
                }
                if ui.button("Cancel").clicked() {
                    self.confirm_clear = false;
                }
            });
        }
        if !ui.ctx().egui_wants_keyboard_input() {
            if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)) {
                self.move_selection(1);
            }
            if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp)) {
                self.move_selection(-1);
            }
        }
        let mut clicked = None;
        let mut send_to = None;
        let list_max = (ui.available_height() - 260.0).max(140.0);
        let list_min = (70.0 + 35.0 * self.visible_ids().len().min(4) as f32).clamp(130.0, 210.0);
        egui::Panel::top("history-list").resizable(true).default_size(220.0).min_size(list_min).max_size(list_max).show(ui,|ui| {
        let rows_height=(ui.available_height()-52.0).max(70.0);
        card().show(ui, |ui| {
            let width = ui.available_width();
            let layout = column_layout(
                &self.features.state.columns,
                &self.features.state.column_widths,
                width,
            );
            let width = layout
                .last()
                .map_or(width, |(_, start, span, _)| start + span)
                .max(width);
            egui::ScrollArea::horizontal()
                .id_salt("history-horizontal")
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.set_width(width);
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(width, 24.0), egui::Sense::hover());
                    for (column, start, span, title) in &layout {
                        let cell = egui::Rect::from_min_size(
                            egui::pos2(rect.left() + start, rect.top()),
                            egui::vec2(*span, 24.0),
                        );
                        let response = ui.interact(
                            egui::Rect::from_min_max(
                                cell.min,
                                egui::pos2(cell.right() - 4.0, cell.bottom()),
                            ),
                            ui.id().with(("sort", *column as u8)),
                            egui::Sense::click(),
                        );
                        if response.clicked() {
                            if self.sort_column == *column {
                                self.sort_descending = !self.sort_descending;
                            } else {
                                self.sort_column = *column;
                                self.sort_descending = false;
                            }
                            self.features.state.filter.sort_by = title.clone();
                            self.features.state.filter.descending = self.sort_descending;
                        }
                        let label = if self.sort_column == *column {
                            format!("{title} {}", if self.sort_descending { "v" } else { "^" })
                        } else {
                            title.clone()
                        };
                        ui.painter()
                            .with_clip_rect(cell.intersect(ui.clip_rect()))
                            .text(
                                egui::pos2(cell.left() + 8.0, cell.center().y),
                                egui::Align2::LEFT_CENTER,
                                label,
                                egui::FontId::proportional(12.0),
                                if response.hovered() { ACCENT } else { MUTED },
                            );
                        response.on_hover_text("Click to sort; click again to reverse");
                        let edge = egui::Rect::from_center_size(
                            egui::pos2(cell.right() - 2.0, cell.center().y),
                            egui::vec2(8.0, 24.0),
                        );
                        let resize = ui
                            .interact(
                                edge,
                                ui.id().with(("resize-column", title)),
                                egui::Sense::click_and_drag(),
                            )
                            .on_hover_cursor(egui::CursorIcon::ResizeHorizontal)
                            .on_hover_text("Drag to resize; double-click for automatic width");
                        if resize.double_clicked() {
                            self.features.state.column_widths.remove(title);
                        } else if resize.dragged() {
                            self.features.state.column_widths.insert(
                                title.clone(),
                                (span + resize.drag_delta().x).clamp(40.0, 2000.0),
                            );
                        }
                        ui.painter().line_segment(
                            [
                                egui::pos2(cell.right() - 2.0, cell.top() + 4.0),
                                egui::pos2(cell.right() - 2.0, cell.bottom() - 4.0),
                            ],
                            egui::Stroke::new(
                                1.0,
                                if resize.hovered() || resize.dragged() {
                                    ACCENT
                                } else {
                                    ui.visuals().widgets.noninteractive.bg_stroke.color
                                },
                            ),
                        );
                    }
                    ui.separator();
                    let ids = self.visible_ids();
                    let height = rows_height;
                    let mut area = egui::ScrollArea::vertical()
                        .id_salt("history-rows")
                        .min_scrolled_height(height)
                        .max_height(height);
                    if self.scroll_selection {
                        if let Some(index) = ids.iter().position(|id| Some(*id) == self.selected) {
                            area = area.vertical_scroll_offset(
                                index as f32 * (28.0 + ui.spacing().item_spacing.y),
                            );
                        }
                        self.scroll_selection = false;
                    }
                    area.show_rows(ui, 28.0, ids.len(), |ui, range| {
                        for row in range {
                            let entry = self
                                .history
                                .entries
                                .iter()
                                .find(|e| e.id == ids[row])
                                .unwrap();
                            let response = traffic_row(
                                ui,
                                entry,
                                &layout,
                                self.features.state.settings.binary_units,
                                self.selected == Some(entry.id)
                                    || self.features.selection.contains(&entry.id),
                            );
                            if response.clicked() {
                                clicked = Some((entry.id, ui.input(|i| i.modifiers)));
                            }
                            response.context_menu(|ui| {
                                if ui.button("Export selected…").clicked() {
                                    if !self.features.selection.contains(&entry.id) {
                                        self.features.selection.clear();
                                        self.features.selection.insert(entry.id);
                                    }
                                    self.features.export_subset = 2;
                                    self.features.settings_section =
                                        features::SettingsSection::Project;
                                    self.workflow.tab = Tab::Settings;
                                    ui.close();
                                }
                                if let Event::Exchange(execution) = &entry.event {
                                    if ui.button("Send to Replay").clicked() {
                                        send_to = Some((entry.id, Tab::Replay));
                                        ui.close();
                                    }
                                    if ui.button("Send to Intruder").clicked() {
                                        send_to = Some((entry.id, Tab::Intruder));
                                        ui.close();
                                    }
                                    if ui.button("Copy URL").clicked() {
                                        ui.ctx().copy_text(execution.parsed.target.clone());
                                        self.toast =
                                            Some(("URL copied".into(), std::time::Instant::now()));
                                        ui.close();
                                    }
                                } else {
                                    ui.label("No HTTP request in this event");
                                }
                            });
                        }
                    });
                    if ids.is_empty() {
                        ui.label(
                            if self.history.entries.is_empty() && self.features.total == 0 {
                                "No captured requests"
                            } else {
                                "No requests in the current view"
                            },
                        );
                        ui.weak(if self.features.total == 0 {
                            "Open browser to capture traffic, or configure a client to use the listener above."
                        } else {
                            "Try a different search or filter, or Refresh to reload saved captures."
                        });
                    }
                });
        });
        });
        if let Some((id, modifiers)) = clicked {
            if modifiers.shift {
                let ids = self.visible_ids();
                let start = ids
                    .iter()
                    .position(|id| Some(*id) == self.selected)
                    .unwrap_or(0);
                let end = ids.iter().position(|i| *i == id).unwrap_or(start);
                for id in &ids[start.min(end)..=start.max(end)] {
                    if self.features.selection.len() < 4096 {
                        self.features.selection.insert(*id);
                    }
                }
            } else if modifiers.ctrl {
                if !self.features.selection.remove(&id) && self.features.selection.len() < 4096 {
                    self.features.selection.insert(id);
                }
            } else {
                self.features.selection.clear();
                self.features.selection.insert(id);
            }
            self.select(id);
        }
        if let Some((id, tab)) = send_to {
            self.transfer_history(id, tab);
        }
        self.inspector(ui);
    }
    fn single_inspector(&mut self, ui: &mut egui::Ui) {
        if self.reset_draft_undo {
            syntax::forget_editor(ui.ctx(), egui::Id::new("history-draft"));
            self.reset_draft_undo = false;
        }
        self.inspector_tools(ui);
        ui.add_space(3.0);
        let transferable = self.history.entries.iter().any(|entry| {
            Some(entry.id) == self.selected && matches!(entry.event, Event::Exchange(_))
        });
        let mut transfer = None;
        ui.horizontal_wrapped(|ui| {
            if let Some(id) = self.selected {
                muted(ui, format!("Request #{id:04}"));
                if let Some(Entry {
                    event: Event::Exchange(execution),
                    ..
                }) = self.history.entries.iter().find(|entry| entry.id == id)
                {
                    let (protocol, stream) = crate::state::protocol_parts(&execution.protocol);
                    if let Some(stream) = stream {
                        muted(ui, format!("{protocol} · Stream ID {stream}"));
                    }
                }
                if !self.visible_ids().contains(&id) {
                    badge(ui, "Selection outside filter", WARN);
                }
            }
            for (tab, label) in [
                (Tab::Replay, "Send to Replay"),
                (Tab::Intruder, "Send to Intruder"),
            ] {
                if ui
                    .add_enabled(transferable, egui::Button::new(label))
                    .clicked()
                {
                    transfer = self.selected.map(|id| (id, tab));
                }
            }
        });
        if let Some((id, tab)) = transfer {
            self.transfer_history(id, tab);
        }
        let previous = (self.pane, self.hex);
        ui.horizontal_wrapped(|ui| {
            for (pane, label) in [
                (Pane::Original, "Original"),
                (Pane::Outgoing, "Outgoing"),
                (Pane::Response, "Response"),
                (Pane::SentResponse, "Sent response"),
                (Pane::Draft, "Draft"),
            ] {
                ui.selectable_value(&mut self.pane, pane, label);
            }
            ui.separator();
            if ui
                .add_enabled(self.selected.is_some(), egui::Button::new("Copy view"))
                .clicked()
            {
                ui.ctx().copy_text(if self.pane == Pane::Draft {
                    self.draft.clone()
                } else {
                    self.text.clone()
                });
                self.toast = Some(("Visible preview copied".into(), std::time::Instant::now()));
            }
        });
        if previous != (self.pane, self.hex) {
            self.refresh();
        }
        if self.pane != Pane::Draft
            && let Some(note) = &self.preview_note
        {
            muted(ui, note);
        }
        if self.pane == Pane::Draft {
            ui.horizontal_wrapped(|ui| {
                badge(ui, "Unsent", WARN);
                muted(
                    ui,
                    "Edits stay here. Selecting another capture resets the draft.",
                );
                if ui.small_button("Reset draft").clicked()
                    && let Some(id) = self.selected
                {
                    self.select(id);
                }
            });
        }
        let editor_id = if self.pane == Pane::Draft {
            egui::Id::new("history-draft")
        } else {
            egui::Id::new(("history-message", self.pane as u8))
        };
        let source = self.history.entries.iter().find_map(|entry| {
            if Some(entry.id) != self.selected {
                return None;
            }
            let Event::Exchange(execution) = &entry.event else {
                return None;
            };
            let wire = match self.pane {
                Pane::Original => Ok(execution.original.0.clone()),
                Pane::Outgoing | Pane::Draft => crate::replay::executed_request(execution),
                _ => return None,
            };
            Some(
                wire.and_then(|wire| crate::replay::origin_draft(&wire))
                    .map(|(destination, request)| syntax::RequestSource {
                        destination,
                        request: if self.pane == Pane::Draft {
                            self.draft.clone()
                        } else {
                            request
                        },
                    })
                    .map_err(|error| error.to_string()),
            )
        });
        syntax::register_request(ui, editor_id, source);
        syntax::wrap_control(ui, editor_id);
        card().show(ui, |ui| {
            let height = (ui.available_height() - 95.0).max(90.0);
            let mut scroll = egui::ScrollArea::both()
                .id_salt(("inspector", self.selected, self.pane as u8, self.hex))
                .min_scrolled_height(height)
                .max_height(height);
            if let Some(line) = self.features.jump.take() {
                scroll = scroll.vertical_scroll_offset(
                    line as f32 * ui.text_style_height(&egui::TextStyle::Monospace),
                );
            }
            scroll.show(ui, |ui| {
                if self.selected.is_none() {
                    ui.label("Select a request to inspect it.");
                } else if self.pane == Pane::Draft {
                    syntax::editor(
                        ui,
                        egui::Id::new("history-draft"),
                        &mut self.draft,
                        true,
                        18,
                    );
                    if self.draft.len() > PREVIEW {
                        let mut end = PREVIEW;
                        while !self.draft.is_char_boundary(end) {
                            end -= 1;
                        }
                        self.draft.truncate(end);
                    }
                } else {
                    {
                        syntax::editor(
                            ui,
                            egui::Id::new(("history-message", self.pane as u8)),
                            &mut self.text,
                            false,
                            18,
                        );
                    }
                }
            });
        });
        muted(
            ui,
            "64 KiB text previews • captured bytes saved in the project",
        );
    }
}
impl eframe::App for Spike {
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        self.poll_project();
        for _ in 0..8 {
            if self.features.importing || self.features.pending.len() >= 16 {
                break;
            }
            match self.rx.try_recv() {
                Ok(event) => self.receive(event),
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    self.disconnected = true;
                    self.workflow.listener_ready = false;
                    break;
                }
                Err(_) => break,
            }
        }
        ui.style_mut().text_styles.insert(
            egui::TextStyle::Monospace,
            egui::FontId::monospace(self.features.state.settings.font_size),
        );
        syntax::set_default_wrap(ui.ctx(), self.features.state.settings.wrap_lines);
        self.capture_notices();
        self.log_panels(ui);
        egui::CentralPanel::default().show(ui, |ui| self.content(ui));
        self.receive_coder_selection(ui.ctx());
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(50));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn history_search_uses_the_same_mime_and_metadata_match_as_saved_pages() {
        let (_tx, rx) = mpsc::channel(1);
        let mut app = Spike::new(rx, "127.0.0.1:8080".parse().unwrap());
        app.receive(exchange());
        app.features.state.filter.text = "stale query".into();
        app.filter = "TEXT/PLAIN".into();
        assert_eq!(app.visible_ids(), vec![1]);
        app.filter = "not present".into();
        assert!(app.visible_ids().is_empty());
        app.filter.clear();
        assert_eq!(app.visible_ids(), vec![1]);
    }
    #[test]
    fn clearing_history_clears_preview_metadata_and_preserves_saved_count() {
        let (_tx, rx) = mpsc::channel(1);
        let mut app = Spike::new(rx, "127.0.0.1:8080".parse().unwrap());
        app.receive(exchange());
        app.features.total = 42;
        app.preview_note = Some("Previous response was binary".into());
        app.decoded_notice = true;
        app.clear();
        assert!(app.selected.is_none());
        assert!(app.text.is_empty() && app.draft.is_empty());
        assert!(app.preview_note.is_none() && !app.decoded_notice);
        assert_eq!(app.features.total, 42);
    }
    #[test]
    fn history_auto_decodes_gzip_and_raw_keeps_original_bytes() {
        use std::io::Write;
        let (_tx, rx) = mpsc::channel(1);
        let mut app = Spike::new(rx, "127.0.0.1:8080".parse().unwrap());
        let Event::Exchange(mut e) = exchange() else {
            unreachable!()
        };
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder
            .write_all(b"window.fixture = 'readable JS';")
            .unwrap();
        e.response = b"HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\n\r\n".to_vec();
        e.response.extend(encoder.finish().unwrap());
        let original = e.response.clone();
        app.receive(Event::Exchange(e));
        app.pane = Pane::Response;
        app.refresh();
        assert!(app.text.contains("window.fixture = 'readable JS';"));
        assert!(app.decoded_notice);
        app.features.auto_decode = false;
        app.refresh();
        assert_eq!(app.text, preview(&original, PREVIEW));
        assert!(!app.decoded_notice);
        let Event::Exchange(e) = &app.history.entries[0].event else {
            unreachable!()
        };
        assert_eq!(e.response, original);
    }
    use crate::model::{OriginalCapture, ParsedMessage, WireExecution};
    fn exchange() -> Event {
        Event::Exchange(WireExecution {
            original: OriginalCapture(b"GET http://local/ HTTP/1.1\r\n\r\n".to_vec()),
            parsed: ParsedMessage {
                method: "GET".into(),
                target: "http://local/".into(),
                headers: vec![],
            },
            outgoing: b"GET / HTTP/1.1\r\n\r\n".to_vec(),
            response_outgoing: vec![],
            protocol: "HTTP/1.1".into(),
            response: b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok".to_vec(),
            run: None,
            transformations: vec!["origin-form".into()],
            response_total_bytes: 40,
            response_status: 200,
            response_truncated: false,
            elapsed_ms: 0.0,
            first_byte_ms: 0.0,
            held_ms: 0.0,
            response_mime: "text/plain".into(),
        })
    }
    #[test]
    fn lossy_preview_respects_byte_limit() {
        let text = preview(&[0xff; 64], 16);
        assert!(text.len() <= 16);
        assert!(text.contains('�'));
        assert_eq!(preview(b"hello", 0), "");
    }
    #[test]
    fn retention_enforces_both_limits_and_oversize_policy() {
        let mut h = History::new(8, 2);
        for message in ["aaaa", "bbbb", "cccc"] {
            h.push(Event::Error(message.into()));
        }
        assert_eq!((h.entries.len(), h.bytes, h.evicted), (2, 8, 1));
        h.push(Event::Error("oversized".into()));
        assert_eq!((h.entries.len(), h.bytes, h.evicted), (2, 8, 2));
        h.push(Event::Error("123456".into()));
        assert_eq!((h.entries.len(), h.bytes, h.evicted), (1, 6, 4));
    }
    #[test]
    fn draft_is_separate_from_captures_and_selection_survives_panes() {
        let (_tx, rx) = mpsc::channel(1);
        let mut app = Spike::new(rx, "127.0.0.1:8080".parse().unwrap());
        app.receive(exchange());
        let original = app.text.clone();
        app.draft = "edited".into();
        app.pane = Pane::Response;
        app.refresh();
        assert!(app.text.ends_with("ok"));
        assert_eq!(app.draft, "edited");
        app.pane = Pane::Original;
        app.refresh();
        assert_eq!(app.text, original);
        app.history.row_limit = 1;
        app.receive(Event::Error("failure".into()));
        assert_eq!(app.selected, Some(1));
        assert_eq!(app.draft, "edited");
        assert_eq!(app.logs.records.len(), 1);
        app.receive(exchange());
        assert!(app.selected.is_none());
        assert!(app.draft.is_empty());
    }
    #[test]
    fn filtering_and_navigation_follow_stable_capture_ids() {
        let (_tx, rx) = mpsc::channel(1);
        let mut app = Spike::new(rx, "127.0.0.1:8080".parse().unwrap());
        app.receive(exchange());
        app.receive(Event::Error("Connection refused".into()));
        app.receive(exchange());
        app.filter = "gEt".into();
        assert_eq!(app.visible_ids(), vec![1, 2]);
        app.move_selection(1);
        assert_eq!(app.selected, Some(2));
        app.move_selection(1);
        assert_eq!(app.selected, Some(2));
        app.move_selection(-1);
        assert_eq!(app.selected, Some(1));
        app.filter.clear();
        assert_eq!(app.visible_ids(), vec![1, 2]);
        assert_eq!(app.logs.records[0].message, "Connection refused");
        app.filter = "REFUSED".into();
        assert!(app.visible_ids().is_empty());
        app.filter = "not found".into();
        app.move_selection(1);
        assert_eq!(app.selected, Some(1));
    }
    #[test]
    fn hex_preview_shows_non_text_bytes_and_marks_truncation() {
        assert!(hex_preview(&[0, 255, b'A']).contains("00 ff 41"));
        let text = hex_preview(&vec![0; 9000]);
        assert!(text.len() < PREVIEW);
        assert!(text.contains("truncated after 8 KiB"));
        assert!(text.contains("00001ff0"));
        assert!(!text.contains("00002000"));
    }
    #[test]
    fn every_workflow_tab_renders_at_compact_size() {
        let (_tx, rx) = mpsc::channel(1);
        let mut app = Spike::new(rx, "127.0.0.1:8080".parse().unwrap());
        let context = egui::Context::default();
        theme(&context);
        app.features.open = true;
        for tab in [
            Tab::Settings,
            Tab::Coder,
            Tab::Compare,
            Tab::Intercept,
            Tab::History,
            Tab::Replay,
            Tab::Intruder,
        ] {
            app.workflow.tab = tab;
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(940.0, 700.0),
                )),
                ..Default::default()
            };
            let mut output = context.run_ui(input, |ui| app.content(ui));
            assert!(!output.shapes.is_empty());
            output.textures_delta.clear();
        }
    }
    #[test]
    fn history_column_sort_is_numeric_reversible_and_preserves_selection() {
        let (_tx, rx) = mpsc::channel(1);
        let mut app = Spike::new(rx, "127.0.0.1:8080".parse().unwrap());
        for (status, size) in [(404, 2000), (200, 30), (500, 100)] {
            let Event::Exchange(mut e) = exchange() else {
                unreachable!()
            };
            e.response_status = status;
            e.response_total_bytes = size;
            app.receive(Event::Exchange(e));
        }
        let selected = app.selected;
        app.sort_column = SortColumn::Size;
        assert_eq!(app.visible_ids(), vec![2, 3, 1]);
        app.sort_descending = true;
        assert_eq!(app.visible_ids(), vec![1, 3, 2]);
        app.sort_column = SortColumn::Status;
        app.sort_descending = false;
        assert_eq!(app.visible_ids(), vec![2, 1, 3]);
        assert_eq!(app.selected, selected);
    }
    #[test]
    fn history_layout_at_supported_sizes() {
        let (_tx, rx) = mpsc::channel(1);
        let mut app = Spike::new(rx, "127.0.0.1:8080".parse().unwrap());
        app.receive(exchange());
        let context = egui::Context::default();
        theme(&context);
        for size in [egui::vec2(1240.0, 860.0), egui::vec2(940.0, 700.0)] {
            app.workflow.tab = Tab::History;
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                ..Default::default()
            };
            let mut output = context.run_ui(input, |ui| app.content(ui));
            assert!(!output.shapes.is_empty());
            output.textures_delta.clear();
        }
    }
}
