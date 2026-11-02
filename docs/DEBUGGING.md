# Diagnostic build

From the diagnostic archive, run `./httpied-debug` in the extracted directory.
From this source workspace, use the command below.

Run the separate executable from a terminal:

```sh
./target/release/httpied-debug
```

It opens the same native application. Trace logging is on by default, including dependency
logs. Startup prints `DEBUG LOG: /absolute/path/...log`. Files are written under
`httpied-debug-logs/` in the launch directory, with timestamps/thread/source locations,
connection IDs and HTTP/2 stream IDs. Close the app normally to flush the writer.

Reproduce the first `tls handshake eof`, then the HTTP/2 timeouts. Keep the active `.log`
and any `.log.1`, `.log.2`, `.log.3` files from that session. The logs distinguish DNS,
individual TCP address attempts, upstream TLS, downstream browser TLS and HTTP/2 execution.
They also record timeouts, durations, errors and whether a stream used the initial upstream
connection. DNS records show whether a lookup was shared or reused from the 30-second cache.
The five-second combined DNS/TCP budget and TLS verification remain in place.
`system resolver job queued`, `system resolver worker started` and worker completion records
separate blocking-pool queue time (`queue_ms`) from OS resolver execution (`worker_ms`). A
late completion is logged even after the request deadline; this matters when the DNS server
reports fast processing but the application still awaits the system resolver.

The current build also writes an application-only `.summary.log`, with its path printed as
`DEBUG SUMMARY`. Include this file when reporting errors: it keeps connection phases and
failures separately from verbose frame/window traces, so those traces cannot rotate away
the first handshake failure from both files. The summary has an independent 8 MiB file limit
and three rotated backups (32 MiB total). Each writer has its own bounded queue.

To choose the log directory:

```sh
HTTPIED_DEBUG_LOG_DIR=/tmp/httpied-debug ./target/release/httpied-debug
```

`RUST_LOG` optionally overrides the default `trace` filter. Logs may contain request metadata
and dependency-level header details; review them before sharing outside the investigation.
Full logging uses an 8,192-entry asynchronous queue, 32 MiB per file and three rotated backups
per session. At shutdown, `DEBUG LOG END` reports dropped entries for both queues and file-write failures.
Separate sessions have separate files; rotated files retain the latest 128 MiB of a session.

Build from source:

```sh
cargo build --locked --release --features ui,debug-logging --bin httpied-debug
```

The normal `httpied` executable does not enable the logging subscriber. Diagnostic logging
adds overhead; use this build to gather evidence, not for performance measurements.
`bash scripts/package-debug-linux.sh` creates a separate Linux archive and checksum.

The bottom **Logs** drawer is available in both builds for application errors/warnings and
brief status events. It retains the latest 500 records in the project and supports search,
severity filters and copying. It is independent of the verbose rotating diagnostic files above;
History no longer displays transport/error rows. Include the diagnostic files when investigating
DNS/TLS/HTTP2 failures, since the drawer does not contain dependency-level traces.
