# Workspace workflow

Run `cargo run --locked --release --features ui`, then create/open a project or choose
Temporary project. The proxy starts after project loading completes. Project directory
paths are entered on the start screen. A saved directory contains `project.sqlite`,
WAL files while open, a lock file and the project's CA files. Two processes cannot open
the same project for writing. Linux creates new project directories/database/key files
with owner-only permissions. Variables and traffic are stored locally without encryption.

## History and Target

Left-click a row to inspect it. Ctrl-click toggles selection; Shift-click selects a range
on the current page. Right-click a loaded HTTP capture to send its executed request to
Replay or Intruder. A disk-backed row loads its bytes when selected.

**Filters / columns** combines extension inclusion/exclusion, MIME, method, status prefix,
minimum duration and minimum size. Use `css, js, png` or `(none)` for extensionless paths.
Filters inspect the path independently from query strings. Save named presets. CONNECT
and opaque-tunnel rows are hidden by default and can be enabled explicitly.

**Target** holds include/exclude domains, IPs, subdomains, schemes, ports and path prefixes.
Exclusions win. `*.example.com` matches child domains; `example.com` with Subdomains also
includes the parent. Domain matching checks label boundaries and normalizes IDNA.
In-scope display changes History visibility. **Restrict Replay and Intruder to scope**
separately enforces scope on every generated request before any connection.

Click column headings to sort the saved result set; sizes/status/timings sort numerically.
Choose columns and reorder them with the adjacent buttons. Size uses decimal B/KB/MB/GB by
default; Settings enables binary KiB/MiB/GiB. An asterisk means capture truncation.
Duration and TTFB are measured; Held records operator wait time. HTTP exchange duration
excludes response interception waits. CONNECT/TLS setup precedes decrypted request timing.

Saved History uses 128 metadata rows per page; bodies load on demand. Live retention defaults
to 128 rows / 16 MiB and is configurable. Evicting or clearing a view does not delete saved
traffic. Project has an explicit confirmed purge action. Disk quota failures are visible;
they do not pretend the capture was saved.

## Inspect and compare

Original, Outgoing, Response and Sent response distinguish observed bytes from serialized
or edited output. HTTP/2 Original is unavailable because raw HPACK/frame bytes are not
retained; Outgoing contains normalized HTTP/1.1. WebSocket captures identify reassembled
messages, direction and close state rather than original frame fragmentation.

Text previews are capped at 64 KiB. Hex displays at most 8 KiB of source bytes. Find / Next
match selects occurrences in text views. Line numbers, HTTP/HTML syntax colors, Pretty
JSON/XML, decoded bodies and parameter/cookie views are display operations. HTML is never
executed. Decoding supports chunked transfer framing and gzip/deflate/Brotli, with a
2 MiB input and 256 KiB expansion limit. A partial or unsupported encoding shows an error.

Set Compare left/right from an inspected view, then open Compare. It shows line differences,
can ignore selected header names and can compare hexadecimal bodies. Comparison is bounded
to 2,048 lines per side and 256 KiB output. Compared text is a preview, not a full binary-file
comparison. Intruder results link to their History captures for the same comparison workflow.

## Replay and Variables

Replay has up to eight independently named, grouped and reorderable sessions. Duplicate
copies the draft. Request options holds a timeout override; 0 uses project settings.
Redirects are not followed automatically. The request/response divider resizes and its
ratio is saved. Response views include raw, pretty, hex and decoded output.

Destination accepts `http://host:port` or `https://host:port`. Edit
`METHOD /path HTTP/1.1` and Send. A missing Host is added; an existing Host must match.
Update Content-Length is on by default. Request edits and latest network results remain
separate. Send bypasses manual interception. Ctrl+Z / Ctrl+Y are text revisions, preserve
redo through cursor movement/sends and are bounded to 64 points / 2 MiB per editor.

Drafts, names, groups and timeout overrides are debounced into the project and flushed on
orderly close. An asterisk indicates a draft differing from its saved snapshot. Closing a
session removes it from recovery. Stored jobs never restart automatically.

Variables is a separate enabled/key/value/secret table. Use `{{base}}`, `{{token}}`, etc.
in the destination, headers or body. Expansion happens once. Missing/duplicate keys stop
the send; values are not recursively interpreted. Resolved request preview masks secrets.
The Secret checkbox masks values in the table; it does not encrypt the project database.

Copy as cURL offers explicit full and redacted variants. Treat an unredacted copy as
containing the request's credentials/body.

## Intruder and Results

Primary sources are text lines, inclusive numeric ranges, UTF-8 wordlists, fixed-length
character sets and seeded reproducible random strings. `0000` through `9999` preserves
padding. Wordlists are capped at 16 MiB / 1,000,000 lines / 4 KiB per line. Browse uses an
owned native picker; path + Load works without a picker. Loading invalidates stale values.

Use `{{payload}}`, `{{payload2}}`, `{{payload3}}`, `{{payload4}}` for 1–4 sets.
Additional sets can use literal lines or reuse the primary set. Sniper varies one set and
uses the configured defaults for inactive positions. Pitchfork zips sets to the shortest.
Cluster bomb forms the product. Prefix/suffix and URL/Base64 transforms are explicit.
Plans validate counts/overflow and generate lazily, capped at 1,000,000 requests.

The plan shows count/destination/delay/concurrency before starting. Settings controls delay,
1–16 concurrent executions, global/per-host rates, retries and 429/503 backoff. UI runs use
one host and apply the stricter global/host rate. Retries are limited to GET/HEAD/OPTIONS,
at most three, and share the attempt rate gate. No redirects or side-effecting-method retries
are automatic. Pause stops new dispatch; active requests finish. Cancel/F9 stops active
sockets, including when result delivery is backpressured. Stop-on-match is optional.

Results persist run/index/payload tuple/status/size/duration/match/error and a capture link.
Refresh and page saved results; filtering searches stored metadata. Heading sorting operates
on the current Results page. Click the index to inspect the linked capture. CSV export
streams all results of the selected run (0 means all). Failed and cancelled executions
retain attribution; a cancellation notification without a dispatched request is not a row.

## Intercept, rules and HTTPS

Intercept is off initially. Toggle it on, then Forward, Forward without edits, or Drop.
Turning off releases held messages without additional manual edits. Holds expire after
60 seconds; editing is capped at 128 KiB and requires lossless UTF-8. Destination changes
are rejected.

Settings conditions select host/path/method and response status/MIME. Ordered rewrites
run before manual editing, preserve original captures and cannot change destinations.
Request replacements must retain valid framing. Response editing repairs Content-Length.
Chunked response text editing is rejected; forward it without edits instead.

Response interception/rewrite buffers at most 128 KiB. Larger responses stream unchanged
and record why those operations were skipped. Ordinary HTTP/1 and HTTP/2 response forwarding
continues beyond capture limits; captured bytes remain bounded.

Enable HTTPS capture in Settings and reopen the project. Manually import its public
`ca.pem` into the test browser/OS trust configuration. Keep `ca.key.pem` private.
Per-host certificates are signed by that project CA. Upstream verification stays enabled
with WebPKI roots. The app never alters OS trust or launches Chromium with certificate or
sandbox bypass flags. A new project has a different CA; reopen preserves existing trust.

HTTP/2 is negotiated over intercepted TLS, with stream IDs and bounded flow control.
Upstream forwarding currently uses HTTP/1.1. Extended CONNECT, h2c, request trailers and raw
HPACK/frame capture are unsupported. WebSocket handshakes and bidirectional messages are
captured; message size is capped at 1 MiB, frame size at 256 KiB, retained message preview at
64 KiB. Compression extensions are not negotiated and message editing is unavailable.

HTTP/1 request chunking, Expect, request pipelining and persistent HTTP/1 connections remain
outside this release. Response Content-Length/chunked/close framing is supported, including
large responses. Default limits: 32 KiB head, 128 headers, 1 MiB request body/capture,
120 s HTTP budget, 10 s read/write idle, 5 s connect and 300 s tunnel idle. Active tunnels
are not limited by the HTTP exchange budget.

## Project exchange, Settings and shortcuts

Project imports JSONL, HAR, raw HTTP or a supported cURL subset. Paste raw/cURL text can load
a Replay draft directly. Imports never run commands, expand local files or send requests.
Imports are capped at 16 MiB and are atomic; invalid input leaves no partially imported rows.
History intake pauses while import assigns new IDs.

JSONL/HAR exports can use all traffic, the current filter, or selected IDs. Existing files
are not overwritten. Redaction removes authentication/cookie headers and bodies; it does
not promise to identify arbitrary secrets in URLs/custom headers. HAR uses decoded bodies,
base64 response content and an HTTPied extension for binary request data. Missing transport
bytes, unavailable timestamps and omitted/un-decodable bodies are annotated.

Settings centralizes listener/timeouts/capture, memory/disk limits, browser, units/font,
scope enforcement, execution, shortcuts and interception rules. Network changes require
reopening the project; rules/display apply live and execution controls apply to new runs.

Defaults: Ctrl+1 History, Ctrl+2 Replay, Ctrl+3 Intruder, Ctrl+4 Intercept, Ctrl+Comma Settings,
Ctrl+Enter sends Replay, F9 stops execution. Ctrl+F searches History; arrows navigate rows.
Shortcut bindings are editable. AccessKit is enabled; screen-reader/IME and Windows/Wayland
interaction coverage remains a platform verification task.
