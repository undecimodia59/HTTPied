//! Opt-in trace logging for the separate diagnostic executable.
use std::{
    fs::{File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};
use tracing_subscriber::fmt::writer::MakeWriterExt;

struct RotatingWriter {
    path: PathBuf,
    file: Option<File>,
    bytes: usize,
    limit: usize,
    errors: Arc<AtomicU64>,
}
fn open(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}
fn backup(path: &Path, index: usize) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(format!(".{index}"));
    name.into()
}
impl RotatingWriter {
    fn new(path: PathBuf, limit: usize, errors: Arc<AtomicU64>) -> io::Result<Self> {
        let file = Some(open(&path)?);
        Ok(Self {
            path,
            file,
            bytes: 0,
            limit,
            errors,
        })
    }
    fn rotate(&mut self) -> io::Result<()> {
        if let Some(mut file) = self.file.take() {
            file.flush()?;
        }
        for index in (1..=3).rev() {
            let source = if index == 1 {
                self.path.clone()
            } else {
                backup(&self.path, index - 1)
            };
            let destination = backup(&self.path, index);
            if index == 3 {
                match std::fs::remove_file(&destination) {
                    Ok(()) => {}
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e),
                }
            }
            match std::fs::rename(source, destination) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
        }
        self.file = Some(open(&self.path)?);
        self.bytes = 0;
        Ok(())
    }
    fn write_inner(&mut self, mut bytes: &[u8]) -> io::Result<usize> {
        let length = bytes.len();
        while !bytes.is_empty() {
            if self.bytes == self.limit {
                self.rotate()?;
            }
            let size = bytes.len().min(self.limit - self.bytes);
            self.file
                .as_mut()
                .ok_or_else(|| io::Error::other("Diagnostic log file unavailable"))?
                .write_all(&bytes[..size])?;
            self.bytes += size;
            bytes = &bytes[size..];
        }
        Ok(length)
    }
}
impl Write for RotatingWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let result = self.write_inner(bytes);
        if let Err(error) = &result
            && self.errors.fetch_add(1, Ordering::Relaxed) == 0
        {
            eprintln!("DEBUG LOG WRITE ERROR: {error}");
        }
        result
    }
    fn flush(&mut self) -> io::Result<()> {
        self.file
            .as_mut()
            .ok_or_else(|| io::Error::other("Diagnostic log file unavailable"))?
            .flush()
    }
}
pub struct Session {
    worker: Option<tracing_appender::non_blocking::WorkerGuard>,
    summary_worker: Option<tracing_appender::non_blocking::WorkerGuard>,
    dropped: tracing_appender::non_blocking::ErrorCounter,
    summary_dropped: tracing_appender::non_blocking::ErrorCounter,
    errors: Arc<AtomicU64>,
}
impl Drop for Session {
    fn drop(&mut self) {
        tracing::info!(
            dropped_lines = self.dropped.dropped_lines(),
            summary_dropped_lines = self.summary_dropped.dropped_lines(),
            write_errors = self.errors.load(Ordering::Relaxed),
            "diagnostic session ending"
        );
        drop(self.worker.take());
        drop(self.summary_worker.take());
        eprintln!(
            "DEBUG LOG END: dropped_lines={} summary_dropped_lines={} write_errors={}",
            self.dropped.dropped_lines(),
            self.summary_dropped.dropped_lines(),
            self.errors.load(Ordering::Relaxed)
        );
    }
}
pub fn start() -> Result<Session, Box<dyn std::error::Error>> {
    let directory = std::env::var_os("HTTPIED_DEBUG_LOG_DIR")
        .map(PathBuf::from)
        .unwrap_or(std::env::current_dir()?.join("httpied-debug-logs"));
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&directory)?;
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
    let path = directory.join(format!(
        "httpied-debug-{timestamp}-{}.log",
        std::process::id()
    ));
    let errors = Arc::new(AtomicU64::new(0));
    let writer = RotatingWriter::new(path.clone(), 32 * 1024 * 1024, errors.clone())?;
    let (writer, worker) = tracing_appender::non_blocking::NonBlockingBuilder::default()
        .buffered_lines_limit(8192)
        .lossy(true)
        .thread_name("httpied-debug-log")
        .finish(writer);
    let dropped = writer.error_counter();
    let summary_path = path.with_extension("summary.log");
    let summary = RotatingWriter::new(summary_path.clone(), 8 * 1024 * 1024, errors.clone())?;
    let (summary, summary_worker) = tracing_appender::non_blocking::NonBlockingBuilder::default()
        .buffered_lines_limit(8192)
        .lossy(true)
        .thread_name("httpied-summary-log")
        .finish(summary);
    let summary_dropped = summary.error_counter();
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("trace"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(false)
        .with_thread_ids(true)
        .with_thread_names(true)
        .with_file(true)
        .with_line_number(true)
        .with_span_events(
            tracing_subscriber::fmt::format::FmtSpan::NEW
                | tracing_subscriber::fmt::format::FmtSpan::CLOSE,
        )
        .with_writer(
            writer.and(summary.with_filter(|metadata| metadata.target().starts_with("httpied"))),
        )
        .try_init()
        .map_err(|error| io::Error::other(error.to_string()))?;
    eprintln!(
        "DEBUG LOG: {} (32 MiB per file, 3 rotated backups)",
        path.display()
    );
    tracing::info!(version=env!("CARGO_PKG_VERSION"),os=std::env::consts::OS,arch=std::env::consts::ARCH,pid=std::process::id(),executable=?std::env::current_exe(),log_path=%path.display(),rust_log=?std::env::var("RUST_LOG").ok(),"diagnostic session started; default filter is trace");
    eprintln!(
        "DEBUG SUMMARY: {} (application events, 8 MiB per file, 3 backups)",
        summary_path.display()
    );
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic| {
        tracing::error!(%panic,backtrace=%std::backtrace::Backtrace::force_capture(),"application panic");
        previous(panic);
    }));
    Ok(Session {
        worker: Some(worker),
        summary_worker: Some(summary_worker),
        dropped,
        summary_dropped,
        errors,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn diagnostic_rotation_bounds_storage_and_retains_latest_bytes() {
        let directory = std::env::temp_dir().join(format!(
            "httpied-log-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("test.log");
        let errors = Arc::new(AtomicU64::new(0));
        let mut writer = RotatingWriter::new(path.clone(), 8, errors.clone()).unwrap();
        writer
            .write_all(b"0123456789abcdefghijklmnopqrstuvwxyzABCD")
            .unwrap();
        writer.flush().unwrap();
        drop(writer);
        assert_eq!(std::fs::read(&path).unwrap(), b"wxyzABCD");
        assert_eq!(std::fs::read(backup(&path, 3)).unwrap(), b"89abcdef");
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 4);
        assert_eq!(errors.load(Ordering::Relaxed), 0);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
