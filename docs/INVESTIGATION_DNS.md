# Supplied session: DNS-stage timeouts

Source: `httpied-debug-1793563502060-71697.log` and its `.3`, `.2`, `.1` rotated files,
read chronologically. Retained interval: 2026-11-01 20:06:15.571–20:07:28.336 UTC.
Shutdown reports zero dropped log entries and zero write errors. Older records were rotated
out; rotation is different from queue overflow.

## Established by the trace

- Eleven failures exhausted the five-second DNS/TCP budget while DNS lookup was pending.
  None of these flows logged a completed DNS lookup or a TCP address attempt.
- Ten failures belong to HTTP/2 connection 17, streams 107, 109, 111, 113, 115, 117, 119,
  121, 123 and 125. One additional failure is CONNECT connection 32.
- Representative stream 107: `.log.1:95828` starts DNS at 20:07:13.864162;
  `.log.1:111087` times out at 20:07:18.864996; `.log.1:111090` resets the stream.
  Connection 32 starts DNS at `.log.1:96072` and times out at `.log.1:111577`.
- Nearby streams to the same lab host resolve in 4–7 ms, connect to IPv4 in 48–52 ms,
  and finish upstream TLS in roughly 97–106 ms. Stream 99 resolves at `.log.1:96144`,
  connects at `96234`, completes TLS at `96545` and completes execution at `99892`.
- Across the retained files, all 29 logged TCP attempts connect successfully. There are
  25 upstream TLS completions and no retained upstream/downstream TLS handshake failure.
- The adapter performs a separate system lookup for each new upstream HTTP/1 connection,
  including concurrent streams for the same HTTP/2 authority.

The observed failure phase is DNS, rather than TCP connection establishment or TLS negotiation.
The trace does not establish whether the resolver delay originates in the local OS resolver,
its upstream DNS service or another external condition. It does not establish TCP address
fallback failure, a broken CA, or server-side connection throttling.

The operator's AdGuard screenshot shows fast server processing: A responses roughly 0.17–0.77 ms
and AAAA responses roughly 30–32 ms. This does not support blaming AdGuard latency. The original
`DNS lookup started` record precedes Tokio's blocking-worker submission; the measured await
includes its queue, OS resolver execution and completion delivery. Those phases were not
separately instrumented, and server processing duration cannot account for their full timeline.
The application's host currently lists two nameservers in resolv.conf; the historical trace
does not identify which handled each DNS transaction. No nameserver/fallback cause is established.

Additional diagnostics now record resolver-job queued, actual worker start (`queue_ms`), and
worker completion/failure (`worker_ms`, `total_ms`). Completion remains logged if an async waiter
has already timed out. Resolution still uses the standard ToSocketAddrs routine on Tokio's
blocking pool, as lookup_host did. A packet-level client send/receive trace would be needed to
separate time inside the OS resolver from DNS transport/retries if worker execution is slow.

AdGuard's [official configuration documentation](https://github.com/AdguardTeam/KnowledgeBaseDNS/blob/master/docs/adguard-home/configuration.md#dns)
also describes a default rate limit of 20 queries/second with excess queries silently dropped.
On 2026-11-02 the operator confirmed the configured value was 20 and changed it to 0.
The log has 17 same-host system lookups starting in 6.144 ms (20:07:13.859636–13.865780 UTC).
Separate A/AAAA queries could exceed the configured rate during that burst. This is consistent
with drops despite fast processing of accepted requests, but historical rate-limit drops have
not been captured or proven. The operator made the setting change; the agent changed no
AdGuard or system DNS settings. Retest outcome with rate limiting disabled is pending.

The previously reported `tls handshake eof` is absent from the retained interval; its origin
cannot be diagnosed from these files. Successful later handshakes do not explain an earlier EOF.

## Implemented response

Share concurrent same-host/port resolution and reuse answers for 30 seconds, bounded to 128
entries and 32 addresses each. Briefly share failed answers (500 ms); keep DNS/TCP deadlines,
sequential address ordering and TLS verification. IP literals bypass system lookup. Invalidate
an answer after every resolved address fails TCP connection. Report DNS-specific deadline
errors before attempting TCP. This reduces duplicate calls; the underlying delay has not
yet been located.

Diagnostic builds additionally retain application events in a separate `.summary.log` with
8 MiB files and three backups. Full dependency TRACE retains its own rotation budget.
This preserves early handshake errors much longer during a verbose browser session.

Tests cover 16 concurrent streams sharing one delayed lookup, cached reuse when a fresh
lookup would hang, cancellation of the first waiter, timeout classification, expiry,
invalidation, bounded entries/answers and IP literals. The verified local HTTPS/HTTP2 fixture
now uses localhost DNS and eight simultaneous additional streams. No public lab requests
were sent during this investigation.
An additional local regression holds the only blocking worker busy and verifies a resolver
deadline expires before a localhost lookup can start, then verifies recovery. This demonstrates
an instrumentation blind spot; it does not establish worker starvation in the supplied session.

Next validation: reproduce using the rebuilt diagnostic executable, then inspect its summary
for DNS sharing/deadlines and, if it recurs, the first upstream/downstream TLS EOF.
