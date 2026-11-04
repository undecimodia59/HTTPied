//! Local integration harness. All certificate/trust files are placed in caller-supplied directories.
#[cfg(feature = "tls-spike")]
fn main() {
    let mut args = std::env::args().skip(1);
    let project = std::path::PathBuf::from(args.next().expect("test project directory"));
    let home = std::path::PathBuf::from(args.next().expect("isolated test home directory"));
    std::fs::create_dir_all(&project).unwrap();
    httpied::authority::Authority::open(&project).unwrap();
    let status = httpied::trust::install(&project, Some(&home)).unwrap();
    println!(
        "{}",
        serde_json::json!({"store":status.store,"fingerprint":status.fingerprint})
    );
}
#[cfg(not(feature = "tls-spike"))]
fn main() {
    panic!("Build this harness with --features tls-spike");
}
