//! External Chromium launcher, managed on a dedicated blocking worker.
use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    time::Duration,
};
type Instance = (Child, PathBuf, Arc<Mutex<Vec<u8>>>);
static NEXT: AtomicU64 = AtomicU64::new(1);
pub enum Action {
    Launch {
        executable: String,
        proxy: SocketAddr,
        url: String,
    },
    Stop,
}
pub struct Service {
    pub commands: mpsc::SyncSender<Action>,
    pub messages: Option<mpsc::Receiver<String>>,
    worker: Option<std::thread::JoinHandle<()>>,
    stop: std::sync::Arc<AtomicBool>,
}
impl Service {
    pub fn start() -> Self {
        let (tx, rx) = mpsc::sync_channel(2);
        let (messages, updates) = mpsc::sync_channel(8);
        let stop_flag = std::sync::Arc::new(AtomicBool::new(false));
        let worker_stop = stop_flag.clone();
        let worker = std::thread::spawn(move || {
            let mut running: Option<Instance> = None;
            loop {
                if worker_stop.load(Ordering::Relaxed) {
                    break;
                }
                if let Some((child, _, _)) = &mut running
                    && let Ok(Some(status)) = child.try_wait()
                {
                    let (_, profile, errors) = running.take().unwrap();
                    let detail = errors
                        .lock()
                        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                        .unwrap_or_default();
                    let _ = std::fs::remove_dir_all(profile);
                    let _ = messages.try_send(format!("Browser exited: {status}. {detail}"));
                }
                match rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(Action::Launch {
                        executable,
                        proxy,
                        url,
                    }) => {
                        if running.is_some() {
                            let _ = messages.try_send(
                                "Browser already running. Close it before launching another."
                                    .into(),
                            );
                            continue;
                        }
                        match launch(&executable, proxy, &url) {
                            Ok(instance) => {
                                running = Some(instance);
                                let _ = messages.try_send(
                                    "Browser started with isolated profile and proxy enabled"
                                        .into(),
                                );
                            }
                            Err(e) => {
                                let _ = messages.try_send(format!("Browser launch failed: {e}"));
                            }
                        }
                    }
                    Ok(Action::Stop) => {
                        stop(&mut running);
                        let _ = messages.try_send(
                            "Browser stopped; temporary profile cleanup attempted".into(),
                        );
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
            }
            stop(&mut running);
        });
        Self {
            commands: tx,
            messages: Some(updates),
            stop: stop_flag,
            worker: Some(worker),
        }
    }
    pub fn shutdown(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        drop(self.commands);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
fn stop(running: &mut Option<Instance>) {
    if let Some((mut child, profile, _)) = running.take() {
        let _ = child.kill();
        let _ = child.wait();
        let _ = std::fs::remove_dir_all(profile);
    }
}
fn executable(custom: &str) -> Result<PathBuf, String> {
    if !custom.trim().is_empty() {
        return Ok(PathBuf::from(custom.trim()));
    }
    if let Some(path) = std::env::var_os("HTTPIED_BROWSER") {
        return Ok(PathBuf::from(path));
    }
    let names = if cfg!(windows) {
        vec!["chrome.exe", "chromium.exe", "msedge.exe"]
    } else {
        vec![
            "chromium",
            "chromium-browser",
            "google-chrome",
            "google-chrome-stable",
        ]
    };
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            for name in &names {
                let candidate = dir.join(name);
                if candidate.is_file() {
                    return Ok(candidate);
                }
            }
        }
    }
    if cfg!(windows) {
        for variable in ["PROGRAMFILES", "PROGRAMFILES(X86)", "LOCALAPPDATA"] {
            if let Some(root) = std::env::var_os(variable) {
                for name in [
                    "Google/Chrome/Application/chrome.exe",
                    "Chromium/Application/chrome.exe",
                    "Microsoft/Edge/Application/msedge.exe",
                ] {
                    let candidate = Path::new(&root).join(name);
                    if candidate.is_file() {
                        return Ok(candidate);
                    }
                }
            }
        }
    }
    Err("Chromium/Chrome was not found. Enter its executable path in the Browser tab.".into())
}
pub fn arguments(proxy: SocketAddr, profile: &Path, url: &str) -> Vec<std::ffi::OsString> {
    vec![
        format!("--proxy-server=http://{proxy}").into(),
        "--proxy-bypass-list=<-loopback>".into(),
        format!("--user-data-dir={}", profile.display()).into(),
        "--no-first-run".into(),
        "--no-default-browser-check".into(),
        "--disable-background-networking".into(),
        "--disable-quic".into(),
        "--new-window".into(),
        url.into(),
    ]
}
fn launch(custom: &str, proxy: SocketAddr, url: &str) -> Result<Instance, String> {
    if !proxy.ip().is_loopback() || proxy.port() == 0 {
        return Err("wait until the loopback proxy is listening".into());
    }
    if url != "about:blank" {
        let uri: http::Uri = url
            .parse()
            .map_err(|_| "start URL must be http://, https:// or about:blank")?;
        if !matches!(uri.scheme_str(), Some("http" | "https")) || uri.host().is_none() {
            return Err("start URL must be http://, https:// or about:blank".into());
        }
    }
    let browser = executable(custom)?;
    let profile = std::env::temp_dir().join(format!(
        "httpied-browser-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&profile).map_err(|e| e.to_string())?;
    let mut command = Command::new(browser);
    command.args(arguments(proxy, &profile, url));
    // Some distro wrappers force Wayland. Respect an explicitly X11-only session.
    if cfg!(target_os = "linux")
        && std::env::var_os("WAYLAND_DISPLAY").is_none()
        && std::env::var_os("DISPLAY").is_some()
    {
        command.arg("--ozone-platform=x11");
    }
    match command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(mut child) => {
            let errors = Arc::new(Mutex::new(Vec::new()));
            let output = errors.clone();
            if let Some(mut pipe) = child.stderr.take() {
                std::thread::spawn(move || {
                    use std::io::Read;
                    let mut buf = [0; 1024];
                    while let Ok(n) = pipe.read(&mut buf) {
                        if n == 0 {
                            break;
                        }
                        if let Ok(mut bytes) = output.lock() {
                            bytes.extend_from_slice(&buf[..n]);
                            if bytes.len() > 4096 {
                                let discard = bytes.len() - 4096;
                                bytes.drain(..discard);
                            }
                        }
                    }
                });
            }
            Ok((child, profile, errors))
        }
        Err(error) => {
            let _ = std::fs::remove_dir_all(profile);
            Err(error.to_string())
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_browser_reports_error_and_worker_shuts_down() {
        let mut service = Service::start();
        let rx = service.messages.take().unwrap();
        service
            .commands
            .send(Action::Launch {
                executable: "/this-browser-does-not-exist/httpied-test".into(),
                proxy: "127.0.0.1:8080".parse().unwrap(),
                url: "about:blank".into(),
            })
            .unwrap();
        let message = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(message.contains("launch failed"));
        service.shutdown();
    }
    #[test]
    fn launch_arguments_keep_sandbox_and_certificate_checks() {
        let args = arguments(
            "127.0.0.1:8080".parse().unwrap(),
            Path::new("profile with spaces"),
            "http://localhost/",
        );
        assert!(args.contains(&"--proxy-bypass-list=<-loopback>".into()));
        assert!(args.contains(&"--user-data-dir=profile with spaces".into()));
        assert!(
            !args
                .iter()
                .any(|arg| arg.to_string_lossy().contains("no-sandbox")
                    || arg.to_string_lossy().contains("ignore-certificate"))
        );
    }
}
