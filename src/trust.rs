//! Explicit operator-requested automatic current-user certificate setup. Never installs private keys.
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
#[derive(Clone, Debug)]
pub struct Status {
    pub store: String,
    pub fingerprint: String,
}
fn run(mut command: Command) -> Result<(), String> {
    use std::io::Read;
    let mut child=command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).spawn().map_err(|e|format!("Certificate setup: {e}. Linux requires NSS certutil (libnss3-tools / mozilla-nss-tools)."))?;
    let stderr = child.stderr.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stderr.take(8192).read_to_end(&mut bytes);
        String::from_utf8_lossy(&bytes).into_owned()
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => {}
            Err(error) => break Err(error.to_string()),
        }
        if start.elapsed() > Duration::from_secs(5) {
            break Err("Certificate setup timed out".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    if status.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    let detail = reader.join().unwrap_or_default();
    let status = status?;
    if !status.success() {
        return Err(format!(
            "Certificate setup failed ({status}): {}",
            detail.trim()
        ));
    }
    Ok(())
}
pub fn install(project: &Path, test_home: Option<&Path>) -> Result<Status, String> {
    let certificate = project.join("ca.der");
    let der = std::fs::read(&certificate).map_err(|e| e.to_string())?;
    if der.len() > 128 * 1024 {
        return Err("CA certificate exceeds 128 KiB".into());
    }
    let fingerprint = format!("{:x}", Sha256::digest(&der));
    #[cfg(target_os = "linux")]
    {
        let home = test_home
            .map(Path::to_path_buf)
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
            .ok_or("Cannot locate current user's home for Chromium trust")?;
        install_nss(&home, &certificate, &fingerprint)
    }
    #[cfg(windows)]
    {
        if test_home.is_some() {
            return Err("Isolated NSS test trust is only available on Linux".into());
        }
        let executable = std::env::var_os("WINDIR")
            .map(PathBuf::from)
            .ok_or("Cannot locate Windows certificate utility")?
            .join("System32/certutil.exe");
        let mut command = Command::new(executable);
        command
            .args(["-user", "-addstore", "-f", "Root"])
            .arg(certificate);
        run(command)?;
        Ok(Status {
            store: "Windows CurrentUser Root".into(),
            fingerprint,
        })
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        let _ = (certificate, test_home);
        Err("Automatic CA trust supports Linux and Windows".into())
    }
}
#[cfg(target_os = "linux")]
fn install_nss(home: &Path, certificate: &Path, fingerprint: &str) -> Result<Status, String> {
    let legacy = home.join(".pki/nssdb");
    let database = if legacy.join("cert9.db").exists() {
        legacy
    } else {
        home.join(".local/share/pki/nssdb")
    };
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    use std::os::unix::fs::DirBuilderExt;
    builder.mode(0o700);
    builder.create(&database).map_err(|e| e.to_string())?;
    let location = format!("sql:{}", database.display());
    if !database.join("cert9.db").exists() {
        let mut command = Command::new("certutil");
        command.args(["-N", "--empty-password", "-d", &location]);
        run(command)?;
    }
    let nickname = format!("HTTPied {fingerprint}");
    let mut command = Command::new("certutil");
    command
        .args(["-A", "-d", &location, "-n", &nickname, "-t", "C,,", "-i"])
        .arg(certificate);
    run(command)?;
    Ok(Status {
        store: database.display().to_string(),
        fingerprint: fingerprint.into(),
    })
}
#[cfg(all(test, feature = "tls-spike", target_os = "linux"))]
mod tests {
    use super::*;
    #[test]
    fn installs_public_ca_idempotently_in_isolated_nss_store() {
        if Command::new("certutil").arg("-H").output().is_err() {
            eprintln!("NSS certutil unavailable; native trust integration requires NSS tools");
            return;
        }
        let path = std::env::temp_dir().join(format!("httpied-trust-test-{}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        let project = path.join("project");
        std::fs::create_dir_all(&project).unwrap();
        crate::authority::Authority::open(&project).unwrap();
        let home = path.join("home");
        let first = install(&project, Some(&home)).unwrap();
        let second = install(&project, Some(&home)).unwrap();
        assert_eq!(first.fingerprint, second.fingerprint);
        let listed = Command::new("certutil")
            .args(["-L", "-d", &format!("sql:{}", first.store)])
            .output()
            .unwrap();
        let listed = String::from_utf8_lossy(&listed.stdout);
        assert_eq!(listed.matches("HTTPied ").count(), 1);
        assert!(listed.contains("C,,"));
        std::fs::remove_dir_all(path).unwrap();
    }
}
