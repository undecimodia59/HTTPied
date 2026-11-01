//! Disk workload evidence, deliberately kept outside the product UI.
use httpied::{
    project::{Command, Reply, Service},
    state::HistoryFilter,
};
use std::time::Instant;
fn main() {
    let count = std::env::args()
        .nth(1)
        .map(|s| s.parse::<u64>().unwrap())
        .unwrap_or(100_000);
    let path = std::env::temp_dir().join(format!("httpied-bench-{}", std::process::id()));
    let start = Instant::now();
    let mut worker = Service::start();
    worker
        .commands
        .send(Command::Open {
            path: path.clone(),
            create: true,
            temporary: false,
            name: "Storage benchmark".into(),
        })
        .unwrap();
    assert!(matches!(
        worker.replies.recv().unwrap(),
        Reply::Opened { .. }
    ));
    let open_ms = start.elapsed().as_secs_f64() * 1000.0;
    let event = httpied::interchange::raw(
        b"GET http://benchmark.test/api/items.json HTTP/1.1\r\nHost: benchmark.test\r\n\r\n",
    )
    .unwrap();
    let start = Instant::now();
    for id in 1..=count {
        worker
            .commands
            .send(Command::Store {
                id,
                event: event.clone(),
            })
            .unwrap();
    }
    worker.finish(std::iter::empty());
    let insert_secs = start.elapsed().as_secs_f64();
    let mut worker = Service::start();
    let start = Instant::now();
    worker
        .commands
        .send(Command::Open {
            path: path.clone(),
            create: false,
            temporary: false,
            name: String::new(),
        })
        .unwrap();
    let Reply::Opened { total, rows, .. } = worker.replies.recv().unwrap() else {
        panic!("reopen failed")
    };
    assert_eq!(total, count);
    assert_eq!(rows.len(), 128.min(count as usize));
    let reopen_ms = start.elapsed().as_secs_f64() * 1000.0;
    let mut pages = Vec::new();
    for _ in 0..20 {
        let start = Instant::now();
        worker
            .commands
            .send(Command::Page {
                offset: count.saturating_sub(128) as usize,
                filter: HistoryFilter::default(),
                scope: vec![],
            })
            .unwrap();
        assert!(matches!(worker.replies.recv().unwrap(), Reply::Page { .. }));
        pages.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    pages.sort_by(f64::total_cmp);
    let rss = std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|text| {
            text.lines()
                .find(|s| s.starts_with("VmRSS:"))
                .map(str::to_owned)
        });
    println!(
        "{}",
        serde_json::json!({"rows":count,"create_ms":open_ms,"insert_secs":insert_secs,"reopen_ms":reopen_ms,"page_median_ms":pages[10],"page_p95_ms":pages[19],"process_rss":rss,"mode":"release, 128 metadata rows per page; warm OS cache; local temporary SQLite; no renderer"})
    );
    worker.finish(std::iter::empty::<Command>());
    std::fs::remove_dir_all(path).unwrap();
}
