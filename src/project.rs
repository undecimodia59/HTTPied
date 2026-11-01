//! Project I/O is serialized on a bounded worker; SQLite metadata and body blobs survive restart.
use crate::{
    model::Event,
    state::{HistoryFilter, ProjectState, ScopeRule},
};
use rusqlite::{Connection, params};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    time::Duration,
};
static TEMP_ID: AtomicU64 = AtomicU64::new(1);
pub enum Command {
    Shutdown,
    InstallTrust{test_home:Option<PathBuf>},
    Purge,
    Result(crate::jobs::Record),
    ExportResults {
        path: PathBuf,
        run_id: u64,
    },
    Results {
        run_id: u64,
        offset: usize,
        query: String,
    },
    Open {
        path: PathBuf,
        create: bool,
        temporary: bool,
        name: String,
    },
    Store {
        id: u64,
        event: Event,
    },
    Save(Box<ProjectState>),
    Page {
        offset: usize,
        filter: HistoryFilter,
        scope: Vec<ScopeRule>,
    },
    Detail(u64),
    Export {
        path: PathBuf,
        format: String,
        redact: bool,
        ids: Option<Vec<u64>>,
        filter: Option<HistoryFilter>,
        scope: Vec<ScopeRule>,
    },
    Import {
        path: PathBuf,
        format: String,
    },
}
pub enum Reply {
    Trust(Result<crate::trust::Status,String>),
    Opened {
        path: PathBuf,
        temporary: bool,
        #[cfg(feature = "tls-spike")]
        ca: std::sync::Arc<crate::authority::Authority>,
        state: Box<ProjectState>,
        max_id: u64,
        total: u64,
        rows: Vec<(u64, Event)>,
    },
    Page {
        offset: usize,
        total: u64,
        rows: Vec<(u64, Event)>,
    },
    Detail {
        id: u64,
        event: Box<Event>,
    },
    Saved,
    StateSaved(Vec<crate::state::SavedRequest>),
    Results {
        rows: Vec<crate::jobs::Record>,
        total: u64,
    },
    Imported {
        count: u64,
        max_id: u64,
        total: u64,
    },
    Stored {
        total: u64,
    },
    Message(String),
    Error(String),
}
pub struct Service {
    pub commands: mpsc::SyncSender<Command>,
    pub replies: mpsc::Receiver<Reply>,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl Service {
    pub fn finish(&mut self, commands: impl IntoIterator<Item = Command>) {
        self.stop.store(true, Ordering::Relaxed);
        for command in commands {
            let _ = self.commands.send(command);
        }
        let _ = self.commands.send(Command::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
    pub fn start() -> Self {
        let (tx, rx) = mpsc::sync_channel(64);
        let (results, updates) = mpsc::sync_channel(64);
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let worker = std::thread::spawn(move || {
            let mut current: Option<Project> = None;
            loop {
                let command = match rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(command) => command,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(_) => continue,
                };
                if matches!(command, Command::Shutdown) {
                    break;
                }
                let outcome = match command {
                    Command::Open {
                        path,
                        create,
                        temporary,
                        name,
                    } => Project::open(path, create, temporary, name).and_then(|project| {
                        let reply = project.opened()?;
                        current = Some(project);
                        Ok(reply)
                    }),
                    command => match &mut current {
                        Some(project) => project.apply(command),
                        None => Err("Open a project first".into()),
                    },
                };
                let reply = outcome.unwrap_or_else(Reply::Error);
                // Capture acknowledgements can be coalesced. Failures/detail results must be delivered.
                if matches!(reply, Reply::Stored { .. } | Reply::Saved) {
                    let _ = results.try_send(reply);
                } else {
                    let mut reply = reply;
                    loop {
                        match results.try_send(reply) {
                            Ok(()) => break,
                            Err(mpsc::TrySendError::Disconnected(_)) => return,
                            Err(mpsc::TrySendError::Full(value)) => {
                                reply = value;
                                if stopping.load(Ordering::Relaxed) {
                                    break;
                                }
                                std::thread::sleep(Duration::from_millis(10));
                            }
                        }
                    }
                }
            }
        });
        Self {
            commands: tx,
            replies: updates,
            stop,
            worker: Some(worker),
        }
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        if self.worker.is_some() {
            self.finish(std::iter::empty());
        }
    }
}
struct Project {
    lock: Option<std::fs::File>,
    db: Option<Connection>,
    path: PathBuf,
    temporary: bool,
    state: ProjectState,
    used: std::cell::Cell<u64>,
    #[cfg(feature = "tls-spike")]
    ca: std::sync::Arc<crate::authority::Authority>,
}
impl Drop for Project {
    fn drop(&mut self) {
        if let Some(db) = self.db.take() {
            let _ = db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
            drop(db);
        }
        self.lock.take();
        if self.temporary {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}
impl Project {
    fn db(&self) -> &Connection {
        self.db.as_ref().unwrap()
    }
    fn open(
        mut path: PathBuf,
        create: bool,
        temporary: bool,
        name: String,
    ) -> Result<Self, String> {
        if temporary {
            path = std::env::temp_dir().join(format!(
                "httpied-project-{}-{}",
                std::process::id(),
                TEMP_ID.fetch_add(1, Ordering::Relaxed)
            ));
        }
        if path.as_os_str().is_empty() {
            return Err("Choose a project directory".into());
        }
        let database = path.join("project.sqlite");
        if create {
            if database.exists() {
                return Err("A project already exists there; use Open".into());
            }
            let mut builder = std::fs::DirBuilder::new();
            builder.recursive(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            builder.create(&path).map_err(|e| e.to_string())?;
        } else if !database.is_file() {
            return Err("No project.sqlite found in this directory".into());
        }
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path.join(".project.lock"))
            .map_err(|e| e.to_string())?;
        fs2::FileExt::try_lock_exclusive(&lock)
            .map_err(|_| "This project is already open in another process".to_owned())?;
        if create {
            let mut options = std::fs::OpenOptions::new();
            options.create_new(true).write(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            options.open(&database).map_err(|e| e.to_string())?;
        }
        let db = Connection::open(&database).map_err(|e| e.to_string())?;
        db.busy_timeout(Duration::from_secs(3))
            .map_err(|e| e.to_string())?;
        let version: u32 = db
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(|e| e.to_string())?;
        if version > 1 {
            return Err("Project schema is newer than this app".into());
        }
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; CREATE TABLE IF NOT EXISTS state(id INTEGER PRIMARY KEY CHECK(id=1),json TEXT NOT NULL); CREATE TABLE IF NOT EXISTS captures(id INTEGER PRIMARY KEY,method TEXT,target TEXT,status INTEGER,duration REAL,bytes INTEGER,meta TEXT NOT NULL,request BLOB,outgoing BLOB,response BLOB);CREATE TABLE IF NOT EXISTS results(run_id INTEGER,idx INTEGER,json TEXT NOT NULL,PRIMARY KEY(run_id,idx));CREATE INDEX IF NOT EXISTS capture_target ON captures(target);CREATE INDEX IF NOT EXISTS capture_status ON captures(status);CREATE INDEX IF NOT EXISTS capture_method ON captures(method);PRAGMA user_version=1;").map_err(|e|e.to_string())?;
        let has_sent: bool = db
            .prepare("PRAGMA table_info(captures)")
            .map_err(|e| e.to_string())?
            .query_map([], |r| r.get::<_, String>(1))
            .map_err(|e| e.to_string())?
            .any(|r| r.is_ok_and(|s| s == "response_sent"));
        if !has_sent {
            db.execute_batch(
                "ALTER TABLE captures ADD COLUMN response_sent BLOB NOT NULL DEFAULT X'';",
            )
            .map_err(|e| e.to_string())?;
        }
        db.execute_batch("UPDATE captures SET method='ERROR' WHERE method='' AND json_type(meta,'$.Error') IS NOT NULL; UPDATE captures SET method='CONNECT' WHERE method='' AND (json_type(meta,'$.Tunnel') IS NOT NULL OR json_type(meta,'$.InterceptedTls') IS NOT NULL);").map_err(|e|e.to_string())?;
        let state = if create {
            ProjectState {
                name: if name.is_empty() {
                    "Untitled".into()
                } else {
                    name
                },
                ..Default::default()
            }
        } else {
            let json: String = db
                .query_row("SELECT json FROM state WHERE id=1", [], |row| row.get(0))
                .map_err(|e| e.to_string())?;
            if json.len() > 4 * 1024 * 1024 {
                return Err("Project settings exceed 4 MiB".into());
            }
            let mut state: ProjectState = serde_json::from_str(&json).map_err(|e| e.to_string())?;
            if state.version != 1 {
                return Err("Unsupported project settings version".into());
            }
            state.migrate_scope();state.validate()?;
            state
        };
        let capture_used:u64=db.query_row("SELECT COALESCE(SUM(length(request)+length(outgoing)+length(response)+length(response_sent)+length(meta)),0) FROM captures",[],|r|r.get(0)).map_err(|e|e.to_string())?;
        let result_used: u64 = db
            .query_row(
                "SELECT COALESCE(SUM(length(json)),0) FROM results",
                [],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        let used = capture_used + result_used;
        if create {
            let json = serde_json::to_string(&state).map_err(|e| e.to_string())?;
            db.execute("INSERT INTO state(id,json) VALUES(1,?1)", [json])
                .map_err(|e| e.to_string())?;
        }
        #[cfg(feature = "tls-spike")]
        let ca = std::sync::Arc::new(
            crate::authority::Authority::open(&path).map_err(|e| e.to_string())?,
        );
        let project = Self {
            lock: Some(lock),
            #[cfg(feature = "tls-spike")]
            ca,
            used: std::cell::Cell::new(used),
            db: Some(db),
            path,
            temporary,
            state,
        };
        project.save()?;
        Ok(project)
    }
    fn save(&self) -> Result<(), String> {
        let json = serde_json::to_string(&self.state).map_err(|e| e.to_string())?;
        self.db().execute("INSERT INTO state(id,json) VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET json=excluded.json",[json]).map_err(|e|e.to_string())?;
        Ok(())
    }
    fn total(&self) -> Result<u64, String> {
        self.db()
            .query_row("SELECT COUNT(*) FROM captures", [], |row| row.get(0))
            .map_err(|e| e.to_string())
    }
    fn opened(&self) -> Result<Reply, String> {
        Ok(Reply::Opened {
            path: self.path.clone(),
            temporary: self.temporary,
            #[cfg(feature = "tls-spike")]
            ca: self.ca.clone(),
            state: Box::new(self.state.clone()),
            max_id: self
                .db()
                .query_row("SELECT COALESCE(MAX(id),0) FROM captures", [], |r| r.get(0))
                .map_err(|e| e.to_string())?,
            total: self.total()?,
            rows: self.page(0, &self.state.filter, &self.state.scope)?.1,
        })
    }
    fn store(&self, id: u64, event: &Event) -> Result<(), String> {
        if matches!(event, Event::Listening(_)) {
            return Ok(());
        }
        let mut meta = event.clone();
        let (mut request, mut outgoing, mut response, mut sent) =
            (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let (mut method, mut target, mut status, mut duration, mut bytes) =
            (String::new(), String::new(), 0, 0.0, 0);
        if let Event::Exchange(e) = &mut meta {
            request = std::mem::take(&mut e.original.0);
            outgoing = std::mem::take(&mut e.outgoing);
            response = std::mem::take(&mut e.response);
            sent = std::mem::take(&mut e.response_outgoing);
            method = e.parsed.method.clone();
            target = e.parsed.target.clone();
            status = e.response_status;
            duration = e.elapsed_ms;
            bytes = e.response_total_bytes;
        }
        match &meta {
            Event::Tunnel { authority, .. } | Event::InterceptedTls { authority, .. } => {
                method = "CONNECT".into();
                target = authority.clone();
            }
            Event::Error(message) => {
                method = "ERROR".into();
                target = message.clone();
            }
            _ => {}
        }
        if let Event::WebSocket {
            payload,
            target: url,
            ..
        } = &mut meta
        {
            request = std::mem::take(payload);
            target = url.clone();
            method = "WS".into();
        }
        let json = serde_json::to_string(&meta).map_err(|e| e.to_string())?;
        let added =
            (request.len() + outgoing.len() + response.len() + sent.len() + json.len()) as u64;
        if self.used.get() + added > self.state.settings.disk_quota_mib * 1024 * 1024 {
            return Err(
                "Project disk quota reached; increase it in Settings. Live forwarding continues."
                    .into(),
            );
        }
        self.db().execute("INSERT INTO captures(id,method,target,status,duration,bytes,meta,request,outgoing,response,response_sent) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",params![id,method,target,status,duration,bytes,json,request,outgoing,response,sent]).map_err(|e|e.to_string())?;
        self.used.set(self.used.get() + added);
        Ok(())
    }
    fn page(
        &self,
        offset: usize,
        filter: &HistoryFilter,
        scope: &[ScopeRule],
    ) -> Result<(u64, Vec<(u64, Event)>), String> {
        let plain = filter.text.is_empty()
            && filter.extensions.is_empty()
            && filter.excluded_extensions.is_empty()
            && filter.mime.is_empty()
            && !filter.in_scope
            && filter.method.is_empty()
            && filter.status.is_empty()
            && filter.min_ms == 0.0
            && filter.min_bytes == 0;
        let column = match filter.sort_by.as_str() {
            "Method" => "method COLLATE NOCASE",
            "Status" => "status",
            "Destination" => "target COLLATE NOCASE",
            "Size" => "bytes",
            "Duration" => "duration",
            "TTFB" => "json_extract(meta,'$.Exchange.first_byte_ms')",
            "Held" => "json_extract(meta,'$.Exchange.held_ms')",
            "MIME" => "json_extract(meta,'$.Exchange.response_mime')",
            "Protocol" => "json_extract(meta,'$.Exchange.protocol')",
            _ => "id",
        };
        let query = format!(
            "SELECT id,meta FROM captures ORDER BY {column} {},id {}",
            if filter.descending { "DESC" } else { "ASC" },
            if filter.descending { "DESC" } else { "ASC" }
        );
        if plain {
            let condition = if filter.errors_only {
                "method='ERROR'"
            } else if !filter.show_tunnels {
                "method<>'CONNECT'"
            } else {
                "1"
            };
            let total = self
                .db()
                .query_row(
                    &format!("SELECT COUNT(*) FROM captures WHERE {condition}"),
                    [],
                    |r| r.get(0),
                )
                .map_err(|e| e.to_string())?;
            let query = query.replacen(
                " FROM captures ORDER BY",
                &format!(" FROM captures WHERE {condition} ORDER BY"),
                1,
            ) + " LIMIT 128 OFFSET ?1";
            let mut statement = self.db().prepare(&query).map_err(|e| e.to_string())?;
            let rows = statement
                .query_map([offset as u64], |r| {
                    Ok((r.get::<_, u64>(0)?, r.get::<_, String>(1)?))
                })
                .map_err(|e| e.to_string())?
                .map(|row| {
                    row.map_err(|e| e.to_string()).and_then(|(id, json)| {
                        serde_json::from_str(&json)
                            .map(|event| (id, event))
                            .map_err(|e| e.to_string())
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            return Ok((total, rows));
        }
        let mut statement = self.db().prepare(&query).map_err(|e| e.to_string())?;
        let mut cursor = statement.query([]).map_err(|e| e.to_string())?;
        let mut count = 0;
        let mut rows = Vec::new();
        while let Some(row) = cursor.next().map_err(|e| e.to_string())? {
            let id: u64 = row.get(0).map_err(|e| e.to_string())?;
            let json: String = row.get(1).map_err(|e| e.to_string())?;
            let event: Event = serde_json::from_str(&json).map_err(|e| e.to_string())?;
            if filter.matches(&event, scope) {
                if count >= offset && rows.len() < 128 {
                    rows.push((id, event));
                }
                count += 1;
            }
        }
        Ok((count as u64, rows))
    }
    fn detail(&self, id: u64) -> Result<Event, String> {
        self.db()
            .query_row(
                "SELECT meta,request,outgoing,response,response_sent FROM captures WHERE id=?1",
                [id],
                |row| {
                    let json: String = row.get(0)?;
                    let mut event: Event = serde_json::from_str(&json)
                        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                    if let Event::Exchange(e) = &mut event {
                        e.original.0 = row.get(1)?;
                        e.outgoing = row.get(2)?;
                        e.response = row.get(3)?;
                        e.response_outgoing = row.get(4)?;
                    }
                    if let Event::WebSocket { payload, .. } = &mut event {
                        *payload = row.get(1)?;
                    }
                    Ok(event)
                },
            )
            .map_err(|e| e.to_string())
    }
    fn apply(&mut self, command: Command) -> Result<Reply, String> {
        match command {
            Command::InstallTrust{test_home}=>Ok(Reply::Trust(crate::trust::install(&self.path,test_home.as_deref()))),
            Command::Purge => {
                self.db().execute_batch("BEGIN IMMEDIATE; DELETE FROM captures; DELETE FROM results; COMMIT; PRAGMA wal_checkpoint(TRUNCATE); VACUUM;").map_err(|e|e.to_string())?;
                self.used.set(0);
                Ok(Reply::Message("Project captures and results purged".into()))
            }
            Command::ExportResults { path, run_id } => {
                use std::io::Write;
                let mut output = std::io::BufWriter::new(
                    std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(path)
                        .map_err(|e| e.to_string())?,
                );
                writeln!(
                    output,
                    "run,index,payloads,status,bytes,elapsed_ms,matched,error"
                )
                .map_err(|e| e.to_string())?;
                let mut statement = self
                    .db()
                    .prepare(
                        "SELECT json FROM results WHERE (?1=0 OR run_id=?1) ORDER BY run_id,idx",
                    )
                    .map_err(|e| e.to_string())?;
                let mut cursor = statement.query([run_id]).map_err(|e| e.to_string())?;
                while let Some(row) = cursor.next().map_err(|e| e.to_string())? {
                    let json: String = row.get(0).map_err(|e| e.to_string())?;
                    let record: crate::jobs::Record =
                        serde_json::from_str(&json).map_err(|e| e.to_string())?;
                    let csv = |s: &str| format!("\"{}\"", s.replace('\"', "\"\""));
                    writeln!(
                        output,
                        "{},{},{},{},{},{},{},{}",
                        record.run_id,
                        record.index + 1,
                        csv(&format!("{:?}", record.payloads)),
                        record.status,
                        record.bytes,
                        record.elapsed_ms,
                        record.matched,
                        csv(&record.error)
                    )
                    .map_err(|e| e.to_string())?;
                }
                output.flush().map_err(|e| e.to_string())?;
                Ok(Reply::Message("Exported Intruder results".into()))
            }
            Command::Result(record) => {
                let json = serde_json::to_string(&record).map_err(|e| e.to_string())?;
                let previous:u64=self.db().query_row("SELECT COALESCE((SELECT length(json) FROM results WHERE run_id=?1 AND idx=?2),0)",params![record.run_id,record.index],|r|r.get(0)).map_err(|e|e.to_string())?;
                let used = self.used.get().saturating_sub(previous) + json.len() as u64;
                if used > self.state.settings.disk_quota_mib * 1024 * 1024 {
                    return Err("Project disk quota reached; result was not saved".into());
                }
                self.used.set(used);
                self.db().execute("INSERT INTO results VALUES(?1,?2,?3) ON CONFLICT(run_id,idx) DO UPDATE SET json=excluded.json",params![record.run_id,record.index,json]).map_err(|e|e.to_string())?;
                Ok(Reply::Saved)
            }
            Command::Results {
                run_id,
                offset,
                query,
            } => {
                let mut statement=self.db().prepare("SELECT json FROM results WHERE (?1=0 OR run_id=?1) ORDER BY run_id DESC,idx").map_err(|e|e.to_string())?;
                let mut cursor = statement.query([run_id]).map_err(|e| e.to_string())?;
                let mut rows = Vec::new();
                let mut total = 0;
                while let Some(row) = cursor.next().map_err(|e| e.to_string())? {
                    let json: String = row.get(0).map_err(|e| e.to_string())?;
                    if !query.is_empty() && !json.to_lowercase().contains(&query.to_lowercase()) {
                        continue;
                    }
                    if total >= offset as u64 && rows.len() < 128 {
                        rows.push(serde_json::from_str(&json).map_err(|e| e.to_string())?);
                    }
                    total += 1;
                }
                Ok(Reply::Results { rows, total })
            }
            Command::Save(state) => {
                state.validate()?;
                self.state = *state;
                self.save()?;
                Ok(Reply::StateSaved(self.state.replays.clone()))
            }
            Command::Store { id, event } => {
                self.store(id, &event)?;
                Ok(Reply::Stored {
                    total: self.total()?,
                })
            }
            Command::Page {
                offset,
                filter,
                scope,
            } => {
                let (total, rows) = self.page(offset, &filter, &scope)?;
                Ok(Reply::Page {
                    offset,
                    total,
                    rows,
                })
            }
            Command::Detail(id) => Ok(Reply::Detail {
                id,
                event: Box::new(self.detail(id)?),
            }),
            Command::Export {
                path,
                format,
                redact,
                ids,
                filter,
                scope,
            } => {
                if !["jsonl", "har"].contains(&format.as_str()) {
                    return Err("Choose JSONL export".into());
                }
                let mut statement = self
                    .db()
                    .prepare("SELECT id FROM captures ORDER BY id")
                    .map_err(|e| e.to_string())?;
                let captures = statement
                    .query_map([], |r| r.get::<_, u64>(0))
                    .map_err(|e| e.to_string())?;
                let file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(path)
                    .map_err(|e| e.to_string())?;
                let mut output = std::io::BufWriter::new(file);
                use std::io::Write;
                if format == "har" {
                    output.write_all(b"{\"log\":{\"version\":\"1.2\",\"creator\":{\"name\":\"HTTPied\",\"version\":\"0.1.0\"},\"entries\":[").map_err(|e|e.to_string())?;
                }
                let mut first = true;
                for id in captures {
                    let id = id.map_err(|e| e.to_string())?;
                    if ids.as_ref().is_some_and(|ids| !ids.contains(&id)) {
                        continue;
                    }
                    let mut event = self.detail(id)?;
                    if filter.as_ref().is_some_and(|f| !f.matches(&event, &scope)) {
                        continue;
                    }
                    if redact {
                        crate::exchange::redact(&mut event);
                    }
                    if format == "jsonl" {
                        serde_json::to_writer(&mut output, &event).map_err(|e| e.to_string())?;
                        output.write_all(b"\n").map_err(|e| e.to_string())?;
                    } else if let Event::Exchange(e) = event {
                        if !first {
                            output.write_all(b",").map_err(|e| e.to_string())?;
                        }
                        first = false;
                        serde_json::to_writer(&mut output, &crate::interchange::har_entry(&e))
                            .map_err(|e| e.to_string())?;
                    }
                }
                if format == "har" {
                    output.write_all(b"]}}").map_err(|e| e.to_string())?;
                }
                output.flush().map_err(|e| e.to_string())?;
                Ok(Reply::Message("Exported captures".into()))
            }
            Command::Import { path, format } => {
                use std::io::Read;
                let mut bytes = Vec::new();
                std::fs::File::open(path)
                    .map_err(|e| e.to_string())?
                    .take(16 * 1024 * 1024 + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|e| e.to_string())?;
                if bytes.len() > 16 * 1024 * 1024 {
                    return Err("Import exceeds 16 MiB".into());
                }
                let mut id: u64 = self
                    .db()
                    .query_row("SELECT COALESCE(MAX(id),0) FROM captures", [], |r| r.get(0))
                    .map_err(|e| e.to_string())?;
                let events: Vec<Event> = match format.as_str() {
                    "har" => crate::interchange::import_har(&bytes)?,
                    "raw" => vec![crate::interchange::raw(&bytes)?],
                    "curl" => vec![crate::interchange::import_curl(
                        std::str::from_utf8(&bytes).map_err(|e| e.to_string())?,
                    )?],
                    "jsonl" => bytes
                        .split(|b| *b == b'\n')
                        .filter(|s| !s.is_empty())
                        .map(serde_json::from_slice)
                        .collect::<Result<_, _>>()
                        .map_err(|e| e.to_string())?,
                    _ => return Err("Choose JSONL, HAR, raw or cURL import".into()),
                };
                if events.iter().any(|e| matches!(e, Event::Listening(_))) {
                    return Err("Import contains a listener event".into());
                }
                let imported = events.len() as u64;
                let used = self.used.get();
                self.db()
                    .execute_batch("BEGIN IMMEDIATE")
                    .map_err(|e| e.to_string())?;
                for event in events {
                    id += 1;
                    if let Err(error) = self.store(id, &event) {
                        let _ = self.db().execute_batch("ROLLBACK");
                        self.used.set(used);
                        return Err(error);
                    }
                }
                self.db()
                    .execute_batch("COMMIT")
                    .map_err(|e| e.to_string())?;
                Ok(Reply::Imported {
                    count: imported,
                    max_id: id,
                    total: self.total()?,
                })
            }
            Command::Shutdown => unreachable!(),
            Command::Open { .. } => unreachable!(),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Event {
        crate::interchange::raw(
            b"GET http://example.test/app.JS?x=.css HTTP/1.1\r\nHost: example.test\r\n\r\n",
        )
        .unwrap()
    }
    #[test]
    fn saved_history_filters_page_sort_bodies_and_import_rollback() {
        let mut project = Project::open(PathBuf::new(), true, true, "Tests".into()).unwrap();
        let path = project.path.clone();
        assert!(Project::open(path.clone(), false, false, String::new()).is_err());
        for id in 1..=260 {
            project.store(id, &fixture()).unwrap();
        }
        let filter = HistoryFilter {
            extensions: "js".into(),
            descending: true,
            ..Default::default()
        };
        let (count, page) = project.page(128, &filter, &[]).unwrap();
        assert_eq!(count, 260);
        assert_eq!(page.len(), 128);
        assert_eq!(page[0].0, 132);
        let Event::Exchange(metadata) = &page[0].1 else {
            panic!()
        };
        assert!(metadata.original.0.is_empty());
        let Event::Exchange(detail) = project.detail(132).unwrap() else {
            panic!()
        };
        assert!(!detail.original.0.is_empty());
        assert_eq!(
            project
                .page(
                    0,
                    &HistoryFilter {
                        extensions: "css".into(),
                        ..Default::default()
                    },
                    &[]
                )
                .unwrap()
                .0,
            0
        );
        let import = path.join("bad.jsonl");
        std::fs::write(
            &import,
            format!(
                "{}\ninvalid json\n",
                serde_json::to_string(&fixture()).unwrap()
            ),
        )
        .unwrap();
        assert!(
            project
                .apply(Command::Import {
                    path: import,
                    format: "jsonl".into()
                })
                .is_err()
        );
        assert_eq!(project.total().unwrap(), 260);
        let export = path.join("selected.jsonl");
        project
            .apply(Command::Export {
                path: export.clone(),
                format: "jsonl".into(),
                redact: false,
                ids: Some(vec![132]),
                filter: None,
                scope: vec![],
            })
            .unwrap();
        assert_eq!(std::fs::read_to_string(export).unwrap().lines().count(), 1);
        project
            .apply(Command::Result(crate::jobs::Record {
                run_id: 5,
                index: 1,
                payloads: vec!["0001".into()],
                capture_id: Some(132),
                ..Default::default()
            }))
            .unwrap();
        let Reply::Results { rows, total } = project
            .apply(Command::Results {
                run_id: 5,
                offset: 0,
                query: "0001".into(),
            })
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(total, 1);
        assert_eq!(rows[0].capture_id, Some(132));
        project.apply(Command::Purge).unwrap();
        assert_eq!(project.total().unwrap(), 0);
    }
    #[test]
    fn project_state_survives_reopen_and_temp_is_removed() {
        let path = std::env::temp_dir().join(format!(
            "httpied-project-test-{}",
            TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let mut project = Project::open(path.clone(), true, false, "Test".into()).unwrap();
        project.state.variables.push(crate::state::Variable {
            key: "host".into(),
            value: "local".into(),
            secret: false,
            enabled: true,
        });
        project.save().unwrap();
        project.store(1, &Event::Error("test".into())).unwrap();
        drop(project);
        let project = Project::open(path.clone(), false, false, String::new()).unwrap();
        assert_eq!(project.state.variables[0].value, "local");
        assert_eq!(project.total().unwrap(), 1);
        drop(project);
        std::fs::remove_dir_all(path).unwrap();
        let project = Project::open(PathBuf::new(), true, true, String::new()).unwrap();
        let path = project.path.clone();
        drop(project);
        assert!(!path.exists());
    }
}
