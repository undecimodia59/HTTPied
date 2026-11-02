//! Shared bounded DNS answers and in-flight lookups; never blocks the network reactor.
use futures_util::{
    FutureExt,
    future::{BoxFuture, Shared},
};
use std::{
    collections::HashMap,
    io,
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tracing::Instrument;
#[derive(Clone, Debug)]
struct Answer {
    finished: Instant,
    value: Result<Arc<[SocketAddr]>, (io::ErrorKind, String)>,
}

struct Entry {
    lookup: Shared<BoxFuture<'static, Answer>>,
    touched: Instant,
}
#[derive(Clone)]
pub struct Resolver {
    entries: Arc<Mutex<HashMap<(String, u16), Entry>>>,
    ttl: Duration,
    capacity: usize,
}
impl std::fmt::Debug for Resolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Resolver")
            .field("ttl", &self.ttl)
            .field("capacity", &self.capacity)
            .finish_non_exhaustive()
    }
}
impl Default for Resolver {
    fn default() -> Self {
        Self {
            entries: Default::default(),
            ttl: Duration::from_secs(30),
            capacity: 128,
        }
    }
}
impl Resolver {
    pub async fn resolve(
        &self,
        host: &str,
        port: u16,
        budget: Duration,
    ) -> io::Result<Arc<[SocketAddr]>> {
        self.resolve_with(host, port, budget, |host, port| async move {
            // Tokio lookup_host delegates to the same blocking ToSocketAddrs call. Expose
            // queue and worker duration separately; log completion even after a waiter times out.
            let queued=Instant::now();
            let span=tracing::Span::current();
            tracing::debug!(%host,port,"system resolver job queued");
            tokio::task::spawn_blocking(move || {
                let _entered=span.enter();
                let started=Instant::now();
                tracing::debug!(%host,port,queue_ms=queued.elapsed().as_millis(),"system resolver worker started");
                let result=std::net::ToSocketAddrs::to_socket_addrs(&(host.as_str(),port))
                    .map(|addresses|addresses.take(32).collect::<Vec<_>>());
                match &result {
                    Ok(addresses)=>tracing::debug!(%host,port,worker_ms=started.elapsed().as_millis(),total_ms=queued.elapsed().as_millis(),?addresses,"system resolver worker completed"),
                    Err(error)=>tracing::warn!(%host,port,worker_ms=started.elapsed().as_millis(),total_ms=queued.elapsed().as_millis(),%error,"system resolver worker failed"),
                }
                result
            }).await.map_err(|error|io::Error::other(format!("System resolver worker: {error}")))?
        })
        .await
    }
    async fn resolve_with<F, Fut>(
        &self,
        host: &str,
        port: u16,
        budget: Duration,
        lookup: F,
    ) -> io::Result<Arc<[SocketAddr]>>
    where
        F: FnOnce(String, u16) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = io::Result<Vec<SocketAddr>>> + Send + 'static,
    {
        if let Ok(ip) = host.parse::<IpAddr>() {
            tracing::debug!(%host,port,"DNS bypassed for IP literal");
            return Ok(Arc::from([SocketAddr::new(ip, port)]));
        }
        let key = (host.to_ascii_lowercase(), port);
        let future = {
            let mut entries = self
                .entries
                .lock()
                .map_err(|_| io::Error::other("DNS resolver lock poisoned"))?;
            let expired = entries
                .get(&key)
                .and_then(|entry| entry.lookup.peek())
                .is_some_and(|answer| {
                    answer.finished.elapsed()
                        >= if answer.value.is_ok() {
                            self.ttl
                        } else {
                            Duration::from_millis(500)
                        }
                });
            if expired {
                entries.remove(&key);
            }
            if let Some(entry) = entries.get_mut(&key) {
                entry.touched = Instant::now();
                tracing::debug!(%host,port,cached=entry.lookup.peek().is_some(),"DNS lookup reused or shared");
                entry.lookup.clone()
            } else {
                if entries.len() >= self.capacity
                    && let Some(oldest) = entries
                        .iter()
                        .min_by_key(|(_, entry)| entry.touched)
                        .map(|(key, _)| key.clone())
                {
                    entries.remove(&oldest);
                }
                let owned_host = key.0.clone();
                let span = tracing::Span::current();
                let future=async move {
                    let started=Instant::now();
                    tracing::trace!(host=%owned_host,port,"DNS lookup started");
                    let result=match tokio::time::timeout(budget,lookup(owned_host.clone(),port)).await {
                        Ok(Ok(addresses)) if !addresses.is_empty()=>Ok(Arc::from(addresses.into_iter().take(32).collect::<Vec<_>>())),
                        Ok(Ok(_))=>Err((io::ErrorKind::AddrNotAvailable,format!("DNS lookup for {owned_host} returned no addresses"))),
                        Ok(Err(error))=>Err((error.kind(),format!("DNS lookup for {owned_host}: {error}"))),
                        Err(_)=>Err((io::ErrorKind::TimedOut,format!("DNS lookup for {owned_host} exceeded {budget:?}; no TCP connection was attempted"))),
                    };
                    match &result {
                        Ok(addresses)=>tracing::debug!(host=%owned_host,port,?addresses,elapsed_ms=started.elapsed().as_millis(),"DNS lookup completed"),
                        Err((kind,error))=>tracing::warn!(host=%owned_host,port,error_kind=?kind,%error,elapsed_ms=started.elapsed().as_millis(),"DNS lookup failed"),
                    }
                    Answer{finished:Instant::now(),value:result}
                }.instrument(span).boxed().shared();
                entries.insert(
                    key.clone(),
                    Entry {
                        lookup: future.clone(),
                        touched: Instant::now(),
                    },
                );
                future
            }
        };
        let answer = future.await;
        answer
            .value
            .map_err(|(kind, message)| io::Error::new(kind, message))
    }
    pub fn invalidate(&self, host: &str, port: u16) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.remove(&(host.to_ascii_lowercase(), port));
        }
        tracing::debug!(%host,port,"DNS cached answer invalidated after TCP failure");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    fn address() -> SocketAddr {
        "127.0.0.1:443".parse().unwrap()
    }
    #[test]
    fn resolver_queue_delay_can_expire_before_system_lookup_runs() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .max_blocking_threads(1)
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let (release, wait) = std::sync::mpsc::channel();
            let (started, ready) = tokio::sync::oneshot::channel();
            let blocker = tokio::task::spawn_blocking(move || {
                let _ = started.send(());
                wait.recv().unwrap();
            });
            ready.await.unwrap();
            let resolver = Resolver::default();
            let result = resolver
                .resolve("localhost", 443, Duration::from_millis(10))
                .await;
            release.send(()).unwrap();
            blocker.await.unwrap();
            let error = result.unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::TimedOut);
            assert!(error.to_string().contains("DNS lookup"));
            resolver.invalidate("localhost", 443);
            assert!(
                !resolver
                    .resolve("localhost", 443, Duration::from_secs(2))
                    .await
                    .unwrap()
                    .is_empty()
            );
        });
    }
    #[tokio::test]
    async fn concurrent_streams_share_one_lookup_and_reuse_its_answer() {
        let resolver = Resolver::default();
        let calls = Arc::new(AtomicUsize::new(0));
        let mut tasks = tokio::task::JoinSet::new();
        for _ in 0..16 {
            let resolver = resolver.clone();
            let calls = calls.clone();
            tasks.spawn(async move {
                resolver
                    .resolve_with(
                        "lab.example",
                        443,
                        Duration::from_secs(1),
                        move |_, _| async move {
                            calls.fetch_add(1, Ordering::Relaxed);
                            tokio::time::sleep(Duration::from_millis(20)).await;
                            Ok(vec![address()])
                        },
                    )
                    .await
                    .unwrap()
            });
        }
        while let Some(result) = tasks.join_next().await {
            assert_eq!(&*result.unwrap(), &[address()]);
        }
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        let cached = resolver
            .resolve_with(
                "LAB.example",
                443,
                Duration::from_millis(10),
                |_, _| async { std::future::pending::<io::Result<Vec<SocketAddr>>>().await },
            )
            .await
            .unwrap();
        assert_eq!(&*cached, &[address()]);
    }
    #[tokio::test]
    async fn cancelling_the_first_waiter_does_not_abandon_shared_resolution() {
        let resolver = Resolver::default();
        let (started, ready) = tokio::sync::oneshot::channel();
        let first = resolver.clone();
        let task = tokio::spawn(async move {
            first
                .resolve_with(
                    "lab.example",
                    443,
                    Duration::from_secs(1),
                    move |_, _| async move {
                        let _ = started.send(());
                        tokio::time::sleep(Duration::from_millis(20)).await;
                        Ok(vec![address()])
                    },
                )
                .await
        });
        ready.await.unwrap();
        task.abort();
        let _ = task.await;
        let result = resolver
            .resolve_with("lab.example", 443, Duration::from_secs(1), |_, _| async {
                panic!("lookup was duplicated")
            })
            .await
            .unwrap();
        assert_eq!(&*result, &[address()]);
    }
    #[tokio::test]
    async fn dns_deadlines_are_explicit_and_briefly_shared() {
        let resolver = Resolver::default();
        let error = resolver
            .resolve_with("lab.example", 443, Duration::from_millis(10), |_, _| {
                std::future::pending::<io::Result<Vec<SocketAddr>>>()
            })
            .await
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(error.to_string().contains("DNS lookup"));
        assert!(error.to_string().contains("no TCP connection"));
        assert!(
            resolver
                .resolve_with("lab.example", 443, Duration::from_secs(1), |_, _| async {
                    panic!("failure was not shared")
                })
                .await
                .is_err()
        );
        resolver.invalidate("lab.example", 443);
        assert!(
            resolver
                .resolve_with("lab.example", 443, Duration::from_secs(1), |_, _| async {
                    Ok(vec![address()])
                })
                .await
                .is_ok()
        );
    }
    #[tokio::test]
    async fn cache_expiry_capacity_and_literals_are_bounded() {
        let resolver = Resolver {
            ttl: Duration::from_millis(5),
            capacity: 2,
            ..Default::default()
        };
        for host in ["a.example", "b.example", "c.example"] {
            resolver
                .resolve_with(host, 443, Duration::from_secs(1), |_, _| async {
                    Ok(vec![address(); 64])
                })
                .await
                .unwrap();
        }
        assert_eq!(resolver.entries.lock().unwrap().len(), 2);
        tokio::time::sleep(Duration::from_millis(10)).await;
        let answer = resolver
            .resolve_with("c.example", 443, Duration::from_secs(1), |_, _| async {
                Ok(vec!["127.0.0.2:443".parse().unwrap()])
            })
            .await
            .unwrap();
        assert_eq!(&*answer, &["127.0.0.2:443".parse::<SocketAddr>().unwrap()]);
        let literal = resolver
            .resolve_with("::1", 8443, Duration::from_secs(1), |_, _| async {
                panic!("literal reached DNS")
            })
            .await
            .unwrap();
        assert_eq!(&*literal, &["[::1]:8443".parse::<SocketAddr>().unwrap()]);
    }
}
