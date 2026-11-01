use httpied::{
    model::Event,
    proxy::{self, Config},
};
use tokio::sync::{mpsc, watch};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut config = Config::default();
    let mut headless = false;
    #[cfg(feature = "tls-spike")]
    let (mut tls_authority, mut ca_out, mut upstream_ca) = (None, None, None);
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--headless" => headless = true,
            #[cfg(feature = "tls-spike")]
            "--tls-intercept" => tls_authority = Some(args.next().ok_or("missing TLS authority")?),
            #[cfg(feature = "tls-spike")]
            "--tls-ca-out" => ca_out = Some(args.next().ok_or("missing CA output path")?),
            #[cfg(feature = "tls-spike")]
            "--tls-upstream-ca" => {
                upstream_ca = Some(args.next().ok_or("missing upstream CA DER path")?)
            }
            "--http-timeout-secs" => {
                config.total_timeout =
                    std::time::Duration::from_secs(args.next().ok_or("missing timeout")?.parse()?)
            }
            "--idle-timeout-secs" => {
                config.idle_timeout = std::time::Duration::from_secs(
                    args.next().ok_or("missing idle timeout")?.parse()?,
                )
            }
            "--capture-mib" => {
                let mib: usize = args.next().ok_or("missing capture size")?.parse()?;
                if !(1..=16).contains(&mib) {
                    return Err("capture size must be 1–16 MiB".into());
                }
                config.max_capture = mib * 1024 * 1024;
            }
            "--listen" => {
                config.listen = args.next().ok_or("--listen requires address")?.parse()?
            }
            "--help" | "-h" => {
                println!(
                    "httpied [--headless] [--listen 127.0.0.1:8080]\nHTTP/1.1 proxy; CONNECT defaults to opaque passthrough."
                );
                println!(
                    "--http-timeout-secs N (default 120)  --idle-timeout-secs N (default 10)  --capture-mib N (default 1, max 16)"
                );
                #[cfg(feature = "tls-spike")]
                println!(
                    "TLS experiment: --tls-intercept HOST:PORT --tls-ca-out NEW_PUBLIC_CA.der [--tls-upstream-ca CA.der]"
                );
                return Ok(());
            }
            _ => return Err(format!("unknown argument: {arg}").into()),
        }
    }
    if config.total_timeout.is_zero()
        || config.idle_timeout.is_zero()
        || config.total_timeout.as_secs() > 86400
        || config.idle_timeout.as_secs() > 86400
    {
        return Err("timeouts must be between 1 and 86400 seconds".into());
    }
    #[cfg(feature = "tls-spike")]
    {
        if let Some(authority) = tls_authority {
            use tokio_rustls::rustls::{RootCertStore, pki_types::CertificateDer};
            let path = ca_out.ok_or("--tls-intercept requires --tls-ca-out (public DER only)")?;
            let mut roots = RootCertStore::empty();
            roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            if let Some(path) = upstream_ca {
                roots.add(CertificateDer::from(std::fs::read(path)?))?;
            }
            let tls = httpied::tls::Intercept::new(authority, roots)?;
            use std::io::Write;
            let mut output = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)?;
            output.write_all(tls.ca_der.as_ref())?;
            eprintln!(
                "TLS_INTERCEPT enabled for {}; ephemeral CA public DER exported",
                tls.authority
            );
            config.tls = Some(std::sync::Arc::new(tls));
        } else if ca_out.is_some() || upstream_ca.is_some() {
            return Err("TLS CA options require --tls-intercept".into());
        }
    }
    let (tx, rx) = mpsc::channel(64);
    let (stop, stop_rx) = watch::channel(false);
    if headless {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?;
        runtime.block_on(async move {
            let listener = proxy::bind(&config).await?;
            println!("READY {}", listener.local_addr()?);
            let consumer = tokio::spawn(consume(rx));
            let signal_stop = stop.clone();
            let signal = tokio::spawn(async move {
                if tokio::signal::ctrl_c().await.is_ok() {
                    let _ = signal_stop.send(true);
                }
            });
            let result = proxy::serve(listener, config, tx, stop_rx).await;
            signal.abort();
            consumer.await.map_err(std::io::Error::other)?;
            result
        })?;
    } else {
        #[cfg(feature = "ui")]
        httpied::ui::run(config, tx, rx, stop, stop_rx)?;
        #[cfg(not(feature = "ui"))]
        return Err("use --headless, or rebuild with --features ui for native UI".into());
    }
    Ok(())
}
async fn consume(mut rx: mpsc::Receiver<Event>) {
    while let Some(event) = rx.recv().await {
        match event {
            Event::Listening(address) => eprintln!("LISTENING {address}"),
            Event::InterceptedTls {
                uploaded,
                downloaded,
                ..
            } => eprintln!("TLS_INTERCEPT up={uploaded} down={downloaded}"),
            Event::WebSocket {
                target,
                direction,
                kind,
                total,
                ..
            } => eprintln!("WS {target} {direction} {kind} {total} bytes"),
            Event::Error(message) => eprintln!("ERROR {message}"),
            Event::Tunnel {
                uploaded,
                downloaded,
                ..
            } => eprintln!("TUNNEL up={uploaded} down={downloaded}"),
            Event::Exchange(e) => eprintln!(
                "HTTP method={} request_bytes={} response_bytes={}",
                e.parsed.method,
                e.original.0.len(),
                e.response.len()
            ),
        }
    }
}
