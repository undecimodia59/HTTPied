//! Lazy, bounded numeric and wordlist payloads; no Cartesian/request materialization.
use std::{
    io,
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
};
pub const MAX_PAYLOADS: usize = 1_000_000;
#[derive(Clone)]
pub enum Payloads {
    Generated {
        alphabet: Arc<Vec<char>>,
        width: usize,
        index: usize,
        count: usize,
        seed: Option<u64>,
    },
    Lines {
        values: Arc<Vec<String>>,
        index: usize,
    },
    Numbers {
        next: u64,
        step: u64,
        width: usize,
        descending: bool,
        remaining: usize,
    },
}
impl Payloads {
    pub fn lines(values: Arc<Vec<String>>) -> io::Result<Self> {
        if values.is_empty() || values.len() > MAX_PAYLOADS {
            return Err(io::Error::other("wordlist needs 1–1,000,000 lines"));
        }
        if values.iter().any(|s| s.len() > 4096) {
            return Err(io::Error::other("a payload exceeds 4 KiB"));
        }
        Ok(Self::Lines { values, index: 0 })
    }
    pub fn numbers(start: &str, end: &str, step: &str) -> io::Result<Self> {
        let parse = |s: &str| -> io::Result<u64> {
            if s.is_empty() || s.len() > 20 || !s.bytes().all(|b| b.is_ascii_digit()) {
                return Err(io::Error::other(
                    "use nonnegative integer bounds (up to 20 digits)",
                ));
            }
            s.parse().map_err(io::Error::other)
        };
        let first = parse(start)?;
        let last = parse(end)?;
        let step = parse(step)?;
        if step == 0 {
            return Err(io::Error::other("sequence step must be greater than zero"));
        }
        let count = (first.abs_diff(last) as u128) / (step as u128) + 1;
        if count > MAX_PAYLOADS as u128 {
            return Err(io::Error::other("sequence exceeds 1,000,000 payloads"));
        }
        let padded = |s: &str| s.len() > 1 && s.starts_with('0');
        let width = if padded(start) || padded(end) {
            start.len().max(end.len())
        } else {
            0
        };
        Ok(Self::Numbers {
            next: first,
            step,
            width,
            descending: first > last,
            remaining: count as usize,
        })
    }
    pub fn generated(
        alphabet: &str,
        width: usize,
        count: Option<usize>,
        seed: Option<u64>,
    ) -> io::Result<Self> {
        let mut chars = Vec::new();
        for ch in alphabet.chars() {
            if !chars.contains(&ch) {
                chars.push(ch);
            }
        }
        if chars.is_empty() || chars.len() > 256 || !(1..=256).contains(&width) {
            return Err(io::Error::other(
                "Use 1–256 characters and a length of 1–256",
            ));
        }
        let count = if seed.is_some() {
            count.unwrap_or(100)
        } else {
            chars
                .len()
                .checked_pow(width as u32)
                .ok_or_else(|| io::Error::other("Character-set product overflow"))?
        };
        if !(1..=MAX_PAYLOADS).contains(&count) {
            return Err(io::Error::other("Generator exceeds 1,000,000 payloads"));
        }
        Ok(Self::Generated {
            alphabet: Arc::new(chars),
            width,
            index: 0,
            count,
            seed,
        })
    }
    pub fn at(&self, index: usize) -> Option<String> {
        if index >= self.len() {
            return None;
        }
        match self {
            Self::Generated {
                alphabet,
                width,
                index: offset,
                seed,
                ..
            } => {
                let mut position = (offset + index) as u64;
                let mut output = vec![' '; *width];
                for (slot, ch) in output.iter_mut().enumerate().rev() {
                    let value = if let Some(seed) = seed {
                        let mut x = seed
                            .wrapping_add(position.wrapping_mul(0x9e3779b97f4a7c15))
                            .wrapping_add(slot as u64);
                        x = (x ^ (x >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
                        x = (x ^ (x >> 27)).wrapping_mul(0x94d049bb133111eb);
                        x ^ (x >> 31)
                    } else {
                        let value = position;
                        position /= alphabet.len() as u64;
                        value
                    };
                    *ch = alphabet[value as usize % alphabet.len()];
                }
                Some(output.into_iter().collect())
            }
            Self::Lines {
                values,
                index: offset,
            } => values.get(offset + index).cloned(),
            Self::Numbers {
                next,
                step,
                width,
                descending,
                ..
            } => {
                let delta = step.checked_mul(index as u64)?;
                let value = if *descending {
                    next.checked_sub(delta)?
                } else {
                    next.checked_add(delta)?
                };
                Some(format!("{value:0width$}", width = *width))
            }
        }
    }
    pub fn len(&self) -> usize {
        match self {
            Self::Generated { index, count, .. } => count - index,
            Self::Lines { values, index } => values.len() - index,
            Self::Numbers { remaining, .. } => *remaining,
        }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
impl Iterator for Payloads {
    type Item = String;
    fn next(&mut self) -> Option<String> {
        if matches!(self, Self::Generated { .. }) {
            let value = self.at(0)?;
            if let Self::Generated { index, .. } = self {
                *index += 1;
            }
            return Some(value);
        }
        match self {
            Self::Generated { .. } => unreachable!(),
            Self::Lines { values, index } => {
                let value = values.get(*index)?.clone();
                *index += 1;
                Some(value)
            }
            Self::Numbers {
                next,
                step,
                width,
                descending,
                remaining,
            } => {
                if *remaining == 0 {
                    return None;
                }
                let value = format!("{:0width$}", *next, width = *width);
                *remaining -= 1;
                if *remaining > 0 {
                    *next = if *descending {
                        next.checked_sub(*step)?
                    } else {
                        next.checked_add(*step)?
                    };
                }
                Some(value)
            }
        }
    }
}
pub fn read_file(path: &Path) -> io::Result<Arc<Vec<String>>> {
    use std::io::Read;
    if !std::fs::metadata(path)?.is_file() {
        return Err(io::Error::other("wordlist is not a regular file"));
    }
    let file = std::fs::File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::other("wordlist is not a regular file"));
    }
    let mut bytes = Vec::new();
    file.take(16 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err(io::Error::other("wordlist exceeds 16 MiB"));
    }
    let text =
        String::from_utf8(bytes).map_err(|_| io::Error::other("wordlist must be UTF-8 text"))?;
    let mut lines = Vec::new();
    for line in text.strip_prefix('\u{feff}').unwrap_or(&text).lines() {
        if line.len() > 4096 || lines.len() >= MAX_PAYLOADS {
            return Err(io::Error::other(
                "wordlist exceeds line count or 4 KiB per-line limit",
            ));
        }
        lines.push(line.to_owned());
    }
    let values = Arc::new(lines);
    Payloads::lines(values.clone())?;
    Ok(values)
}
pub enum FileAction {
    Browse,
    Load(PathBuf),
}
pub struct FileResult {
    pub path: Option<PathBuf>,
    pub result: Result<Option<Arc<Vec<String>>>, String>,
}
pub struct FileWorker {
    pub commands: mpsc::SyncSender<FileAction>,
    pub results: mpsc::Receiver<FileResult>,
    stop: Arc<std::sync::atomic::AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl Drop for FileWorker {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
impl FileWorker {
    pub fn start() -> Self {
        let (tx, rx) = mpsc::sync_channel(1);
        let (results, updates) = mpsc::sync_channel(1);
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stopped = stop.clone();
        let worker = std::thread::spawn(move || {
            loop {
                if stopped.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                let action = match rx.recv_timeout(std::time::Duration::from_millis(100)) {
                    Ok(action) => action,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(_) => continue,
                };
                let path = match action {
                    FileAction::Load(path) => Ok(Some(path)),
                    FileAction::Browse => pick_file(&stopped),
                };
                let (path, result) = match path {
                    Ok(Some(path)) => {
                        let result = read_file(&path).map(Some).map_err(|e| e.to_string());
                        (Some(path), result)
                    }
                    Ok(None) => (None, Ok(None)),
                    Err(error) => (None, Err(error.to_string())),
                };
                let mut value = FileResult { path, result };
                loop {
                    match results.try_send(value) {
                        Ok(()) => break,
                        Err(mpsc::TrySendError::Disconnected(_)) => return,
                        Err(mpsc::TrySendError::Full(v)) => {
                            value = v;
                            if stopped.load(std::sync::atomic::Ordering::Relaxed) {
                                return;
                            }
                            std::thread::sleep(std::time::Duration::from_millis(20));
                        }
                    }
                }
            }
        });
        Self {
            commands: tx,
            results: updates,
            stop,
            worker: Some(worker),
        }
    }
}
fn pick_file(stop: &std::sync::atomic::AtomicBool) -> io::Result<Option<PathBuf>> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    let spawn = |command: &mut Command| {
        if cfg!(target_os = "linux")
            && std::env::var_os("WAYLAND_DISPLAY").is_none()
            && std::env::var_os("DISPLAY").is_some()
        {
            command
                .env("QT_QPA_PLATFORM", "xcb")
                .env("GDK_BACKEND", "x11");
        }
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
    };
    let mut child = if cfg!(windows) {
        spawn(Command::new("powershell.exe").args(["-NoProfile","-STA","-Command","Add-Type -AssemblyName System.Windows.Forms; $d=New-Object System.Windows.Forms.OpenFileDialog; $d.Title='Open wordlist'; $d.Filter='Text files|*.txt;*.lst;*.csv|All files|*.*'; if ($d.ShowDialog() -eq 'OK') { [Console]::Write($d.FileName) }"]))
    } else {
        spawn(Command::new("kdialog").args([
            "--title",
            "Open wordlist",
            "--getopenfilename",
            ".",
            "*",
        ]))
        .or_else(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                spawn(Command::new("zenity").args(["--file-selection", "--title=Open wordlist"]))
            } else {
                Err(error)
            }
        })
    }?;
    let status = loop {
        if stop.load(std::sync::atomic::Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(None);
        }
        if let Some(status) = child.try_wait()? {
            break status;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    if !status.success() {
        if status.code() == Some(1) {
            return Ok(None);
        }
        return Err(io::Error::other(
            "file picker failed; enter the path and click Load",
        ));
    }
    let mut bytes = Vec::new();
    if let Some(stdout) = child.stdout.take() {
        stdout.take(16385).read_to_end(&mut bytes)?;
    }
    if bytes.len() > 16384 {
        return Err(io::Error::other("file picker returned an oversized path"));
    }
    let text = String::from_utf8(bytes).map_err(|_| io::Error::other("file path is not UTF-8"))?;
    let path = text.trim_end_matches(['\r', '\n']);
    Ok(if path.is_empty() {
        None
    } else {
        Some(path.into())
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generated_sources_are_reproducible_and_bounded() {
        let charset = Payloads::generated("ab", 2, None, None)
            .unwrap()
            .collect::<Vec<_>>();
        assert_eq!(charset, vec!["aa", "ab", "ba", "bb"]);
        let random = Payloads::generated("0123456789abcdef", 8, Some(1_000_000), Some(42)).unwrap();
        assert_eq!(random.len(), 1_000_000);
        assert_eq!(random.at(1234), random.clone().nth(1234));
        assert_eq!(
            random.at(1234),
            Payloads::generated("0123456789abcdef", 8, Some(1_000_000), Some(42))
                .unwrap()
                .at(1234)
        );
        assert!(Payloads::generated("ab", 64, None, None).is_err());
    }
    #[test]
    fn inclusive_padded_sequences_are_lazy() {
        let mut p = Payloads::numbers("0000", "9999", "1").unwrap();
        assert_eq!(p.len(), 10000);
        assert_eq!(p.next().unwrap(), "0000");
        assert_eq!(p.next().unwrap(), "0001");
        assert_eq!(p.last().unwrap(), "9999");
        assert_eq!(Payloads::numbers("0", "100", "1").unwrap().count(), 101);
    }
    #[test]
    fn descending_steps_and_overflow_are_checked() {
        assert_eq!(
            Payloads::numbers("005", "001", "2")
                .unwrap()
                .collect::<Vec<_>>(),
            ["005", "003", "001"]
        );
        assert!(Payloads::numbers("0", "2", "0").is_err());
        assert!(Payloads::numbers("0", "99999999999", "1").is_err());
        assert_eq!(
            Payloads::numbers("18446744073709551615", "18446744073709551615", "1")
                .unwrap()
                .next()
                .unwrap(),
            "18446744073709551615"
        );
    }
}

#[cfg(test)]
mod file_tests {
    use super::*;
    #[test]
    fn wordlists_preserve_utf8_padding_and_empty_payloads() {
        let path =
            std::env::temp_dir().join(format!("httpied-wordlist-{}.txt", std::process::id()));
        std::fs::write(&path, "\u{feff}0001\r\nü\r\n\r\n0002\n").unwrap();
        let values = read_file(&path).unwrap();
        assert_eq!(&**values, &["0001", "ü", "", "0002"]);
        std::fs::write(&path, [0xff, 0xfe]).unwrap();
        assert!(read_file(&path).is_err());
        std::fs::write(&path, "x".repeat(4097)).unwrap();
        assert!(read_file(&path).is_err());
        std::fs::remove_file(&path).unwrap();
    }
}
