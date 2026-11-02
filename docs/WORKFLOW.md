# Workspace workflow

Run `cargo run --locked --release --features ui`, then create/open a project or choose
Temporary session. The proxy starts after project loading completes. Project directory
paths are entered on the start screen. A saved directory contains `project.sqlite`,
WAL files while open, a lock file and the project's CA files. Two processes cannot open
the same project for writing. Linux creates new project directories/database/key files
with owner-only permissions. Variables and traffic are stored locally without encryption.
Opening older projects upgrades capture metadata to schema 2 for indexed Host/Endpoint
sorting; executables limited to schema 1 cannot reopen the upgraded database.

## Current CTF workspace

The app opens on History. Select a row to inspect its **Executed request** and **Response**
side by side. Drag the list/inspector separator or the request/response separator to resize.
**Send to Replay** (Ctrl+R) is the primary transfer; **More** includes Intruder and comparison
operations. Other captured byte variants and the unsent scratch draft remain under
**More → Capture variants & draft**. **View** groups column layout, reload and clear-live-view actions.

Replay has a session strip, a destination/send bar, **Session** management and **Request actions**.
Copy-as-cURL variants, Content-Length repair, group and timeout controls are in Request actions.
Response modes use a dropdown; response actions include copying and sending to Compare.
Ctrl+Enter sends; F9 stops execution. Intruder uses an expandable payload configuration and a
resizable results pane. The global header owns browser start/stop; Browser settings configure it.

Settings groups Scope, Variables and project files; execution/interception; connection; and
appearance/shortcuts. Variable secret/enabled/remove controls are under each row's Options menu.
Project settings separate the import format from JSONL/HAR export choices; project changes autosave.

### Coder and Compare

Select text by dragging or Shift+arrows in a message editor and right-click **Send selection to Coder**.
Ctrl+5 opens Coder directly. Choose Base64, Base64url, URL percent, URL form (+), HTML entities or
Hex, then Encode/Decode. URL percent preserves literal `+`; form decoding treats it as a space.
Base64 decoding accepts whitespace and optional padding. HTML entities require terminating `;`.
Input is UTF-8 text unless **Interpret as hex bytes** is selected. Decoded binary remains exact bytes
and defaults to Hex. Copy exports the complete result; previews are limited to 64 KiB of text.
**Use as input** chains a result when it fits the 128 KiB input budget. Maximum conversion output is
1 MiB. Coder input/results are cleared when the app exits and are not persisted to the project.

In any message editor, **Set Compare baseline** records the current message; **Compare with baseline**
records the candidate and opens Compare. Ctrl+6 also opens Compare, where you can paste or edit
both sides. **Compare inputs** switches to aligned colored results: baseline removals/changes are red,
candidate additions/changes green. Scrolling is synchronized; Previous/Next change moves through
changed rows. **Edit inputs**, **Swap sides** and header/hex Options are available in the toolbar.
Comparison is bounded to 2,048 lines and 256 KiB per side; oversized inputs produce a visible limit.

## History and Scope

Left-click a row to inspect it. Ctrl-click toggles selection; Shift-click selects a range
on the current page. Right-click an HTTP capture to send its executed request to
Replay or Intruder. A disk-backed row loads its bytes when selected.

**Filters** opens extension inclusion/exclusion, MIME, method, status prefix,
minimum duration and minimum size controls. **View → Choose columns…** opens a separate ordered column list.
Use `css, js, png` or `(none)` for extensionless paths.
Filters inspect the path independently from query strings. Save named presets. CONNECT
and opaque-tunnel rows are hidden by default and can be enabled explicitly.

**Settings → Scope** holds include/exclude domains, IPs, subdomains, schemes, ports and path prefixes.
Exclusions win. `*.example.com` matches child domains; `example.com` with Subdomains also
includes the parent. Domain matching checks label boundaries and normalizes IDNA.
New entries include subdomains by default: `web-security-academy.net` includes its lab hosts.
Older display-only rules migrate to this behavior; enforced execution scopes retain their
existing exact-host boundaries. Uncheck Subdomains for exact-host matching.
History's **In scope** applies the current scope immediately, including to saved traffic.
Editing scope does not hide History entries while this flag is off; there is no Apply button.
**Restrict Replay and Intruder to scope**
separately enforces scope on every generated request before any connection.

Click column headings to sort the saved result set; sizes/status/timings sort numerically.
Host is the hostname, Endpoint is the encoded path and query, and Destination is the complete
target URL, including scheme and port. Enable any combination in Columns; saved layouts keep
their existing selection. New projects show Host and Endpoint by default.
Drag a header's right edge to resize its column. Double-click the edge to restore automatic
width, or use Columns → Reset all widths. Widths survive hiding/reordering columns and project
reopening. Wide tables scroll horizontally with headers and rows aligned.
Choose columns and reorder them with the adjacent buttons. Size uses decimal B/KB/MB/GB by
default; Settings enables binary KiB/MiB/GiB. An asterisk means capture truncation.
Duration and TTFB are measured; Held records operator wait time. HTTP exchange duration
excludes response interception waits. CONNECT/TLS setup precedes decrypted request timing.

Saved History uses 128 metadata rows per page; bodies load on demand. Live retention defaults
to 128 rows / 16 MiB and is configurable. Evicting or clearing a view does not delete saved
traffic. Settings → Files & storage has an explicit confirmed purge action. Disk quota failures are visible;
they do not pretend the capture was saved.

## Inspect and compare

Original, Outgoing, Response and Sent response distinguish observed bytes from serialized
or edited output. HTTP/2 Original is unavailable because raw HPACK/frame bytes are not
retained; Outgoing contains normalized HTTP/1.1. WebSocket captures identify reassembled
messages, direction and close state rather than original frame fragmentation.

Text previews are capped at 64 KiB. Hex displays at most 8 KiB of source bytes. Find / Next
match selects occurrences in text views. Line numbers, HTTP/HTML syntax colors, Pretty
JSON/XML, decoded bodies and parameter/cookie views are display operations. HTML is never
executed. Decoding supports chunked transfer framing and gzip/deflate/Brotli/Zstandard, with a
2 MiB input and 256 KiB expansion limit. A partial or unsupported encoding shows an error.
Auto is the default History and Replay/Intruder response view: compressed/chunked bodies
are decoded for reading, including gzip JavaScript. Captured headers stay visible and a note
identifies the decoded body. Raw and Hex show the encoded captured bytes. Pretty also decodes
before formatting. Unsupported encodings, binary decoded data, truncated
streams or expansion-limit failures show a decoding message with Raw/Hex still available.

Set a Compare baseline and candidate from a message editor, or paste both inputs directly. Compare shows aligned, highlighted line differences,
can ignore selected header names and can compare hexadecimal bodies. Comparison is bounded
to 2,048 lines per side and 256 KiB output. Compared text is a preview, not a full binary-file
comparison. Intruder results link to their History captures for the same comparison workflow.

## Replay and Variables

Replay has up to eight independently named, grouped and reorderable sessions. Duplicate
copies the draft. Request actions holds a timeout override; 0 uses project settings.
Redirects are not followed automatically. The request/response divider resizes and its
ratio is saved. Response views include raw, pretty, hex and decoded output.
The initial response view is Auto; changing views never changes forwarded or saved bytes.

Destination accepts `http://host:port` or `https://host:port`. Edit
`METHOD /path HTTP/1.1` and Send. A missing Host is added; an existing Host must match.
Update Content-Length is on by default. Request edits and latest network results remain
separate. Send bypasses manual interception. Ctrl+Z / Ctrl+Y are text revisions, preserve
redo through cursor movement/sends and are bounded to 64 points / 2 MiB per editor.

Drafts, names, groups and timeout overrides are debounced into the project and flushed on
orderly close. A dot beside the session name indicates a draft differing from its saved snapshot. Closing a
session removes it from recovery. Stored jobs never restart automatically.

Settings → Variables contains key/value rows; each row’s Options menu controls enabled/secret state. Use `{{base}}`, `{{token}}`, etc.
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

The source shows its payload count; Attack options holds modes, positions and transforms.
Settings → Execution limits controls delay,
1–16 concurrent executions, global/per-host rates, retries and 429/503 backoff. UI runs use
one host and apply the stricter global/host rate. Retries are limited to GET/HEAD/OPTIONS,
at most three, and share the attempt rate gate. No redirects or side-effecting-method retries
are automatic. Pause stops new dispatch; active requests finish. Stop/F9 stops active
sockets, including when result delivery is backpressured. Stop-on-match is optional.

Results appear below the Intruder editors and persist run/index/payload tuple/status/size/
duration/match/error and a capture link. They refresh as requests finish. Search payload/error
text, or use Filter for latest-run/all-run, status prefix, errors and matches. Heading sorting
operates over the entire selected saved result set before paging. Click the index to inspect
the linked capture. CSV export streams all results of the selected run (0 means all).
Failed and cancelled executions
retain attribution; a cancellation notification without a dispatched request is not a row.

## Intercept, rules and HTTPS

Intercept is off initially. Toggle it on, then Forward, Discard edits & forward, or Drop.
Turning off releases held messages without additional manual edits. Holds expire after
60 seconds; editing is capped at 128 KiB and requires lossless UTF-8. Destination changes
are rejected.

Settings → Interception conditions select host/path/method and response status/MIME.
Input hints show accepted formats: host `example.com` or `*.example.com`, path prefix `/api/`,
one method such as `GET`, status prefix `200`, `4` or `5`, and MIME substring `application/json`.
Ordered rewrites
run before manual editing, preserve original captures and cannot change destinations.
Request replacements must retain valid framing. Response editing repairs Content-Length.
Chunked response text editing is rejected; forward it without edits instead.

Response interception/rewrite buffers at most 128 KiB. Larger responses stream unchanged
and record why those operations were skipped. Ordinary HTTP/1 and HTTP/2 response forwarding
continues beyond capture limits; captured bytes remain bounded.

HTTPS interception is always enabled in the desktop app. On project opening, a bounded
storage worker installs only its public CA into the current user's Chromium NSS store on
Linux or CurrentUser Root on Windows. Linux requires NSS `certutil`. Existing legacy
`~/.pki/nssdb` databases are respected; otherwise `~/.local/share/pki/nssdb` is used.
Settings → Certificates shows setup status, SHA-256 fingerprint, public certificate path
and a retry action. The browser launch button waits for successful setup and listener startup.
Other browser certificate stores need the public `ca.pem` imported separately.
Keep `ca.key.pem` private. Per-host certificates are signed by that project CA. Upstream
verification uses WebPKI roots; Chromium's certificate verification and sandbox stay enabled.
A new project has a different CA; reopening preserves it. Installed public roots remain in
the current-user store after closing or deleting a project, including temporary projects.
The headless CLI retains its explicit TLS configuration.

HTTP/2 is negotiated over intercepted TLS, with stream IDs and bounded flow control.
Split HTTP/2 Cookie fields are joined with `; ` before HTTP/1 forwarding; the conversion is
annotated in captures. This preserves cookie pair order and does not merge response Set-Cookie
fields or rewrite intentional duplicates in raw HTTP/1 requests.
Upstream forwarding currently uses HTTP/1.1. Extended CONNECT, h2c, request trailers and raw
HPACK/frame capture are unsupported. WebSocket handshakes and bidirectional messages are
captured; message size is capped at 1 MiB, frame size at 256 KiB, retained message preview at
64 KiB. Compression extensions are not negotiated and message editing is unavailable.

HTTP/1 request chunking, Expect, request pipelining and persistent HTTP/1 connections remain
outside this release. Response Content-Length/chunked/close framing is supported, including
large responses. Default limits: 32 KiB head, 128 headers, 1 MiB request body/capture,
120 s HTTP budget, 10 s read/write idle, 5 s connect and 300 s tunnel idle. Active tunnels
are not limited by the HTTP exchange budget.

DNS resolution is shared across concurrent requests for the same host/port. Successful
answers are reused for 30 seconds; failed lookups are shared for 500 ms. The resolver retains
at most 128 entries and 32 addresses per answer. IP literals bypass DNS; all-address TCP
failure or a TCP deadline invalidates the cached answer. DNS and TCP share the configured
connect budget. Error messages distinguish DNS expiry before TCP from a TCP connection failure.

## Project exchange, Settings and shortcuts

Settings → Files & storage imports JSONL, HAR, raw HTTP or a supported cURL subset. Paste raw/cURL text can load
a Replay draft directly. Imports never run commands, expand local files or send requests.
Imports are capped at 16 MiB and are atomic; invalid input leaves no partially imported rows.
History intake pauses while import assigns new IDs.

JSONL/HAR exports can use all traffic, the current filter, or selected IDs. Existing files
are not overwritten. Redaction removes authentication/cookie headers and bodies; it does
not promise to identify arbitrary secrets in URLs/custom headers. HAR uses decoded bodies,
base64 response content and an HTTPied extension for binary request data. Missing transport
bytes, unavailable timestamps and omitted/un-decodable bodies are annotated.

Settings uses a sidebar with Network, Display & storage, Execution, Scope, Variables,
Interception, Shortcuts, Browser, Certificates and Project subsections. Network changes require
reopening the project; rules/display apply live and execution controls apply to new runs.

Defaults: Ctrl+1 History, Ctrl+2 Replay, Ctrl+3 Intruder, Ctrl+4 Intercept, Ctrl+Comma Settings,
Ctrl+Enter sends Replay, F9 stops execution. Ctrl+F searches History; arrows navigate rows.
Shortcut bindings are editable. AccessKit is enabled; screen-reader/IME and Windows/Wayland
interaction coverage remains a platform verification task.

### Logs and automatic response decoding

Click **Logs** at the bottom to open a resizable drawer. Search, filter Info/Warnings/Errors,
copy visible entries, or clear them independently of traffic. The project retains the latest
500 entries, at most 4 KiB per message. Consecutive identical entries within five seconds
coalesce with a repeat count. UTC times are recorded; migrated older diagnostics have no
original timestamp. Existing Error records move to this view without deleting their originals.
HTTP 4xx/5xx responses stay in History; its HTTP errors preset selects those response statuses.
Execution failures also keep their per-run Intruder result and appear in Logs.

The Protocol column displays **HTTP/2**, while optional **Stream ID** shows its multiplexed
stream number. This is connection-local metadata, not a protocol version or globally unique ID.

History and Replay **Auto** decode gzip/x-gzip, zlib/raw deflate, Brotli, Zstandard and chunked
framing. Repeated/comma-separated content encodings decode in reverse order, with up to four
content and four transfer layers. Gzip/Zstandard magic can be recognized without an encoding
header in display views. Declared text charsets and UTF-16 BOMs are supported. Readable text
is displayed; binary, corrupt, unsupported or oversized content keeps the captured byte preview
and an explanatory note. **Decoded** reports failures explicitly. Headers retain their captured
values, so a displayed Content-Length may describe the compressed bytes. These views do not
recompress outgoing traffic or modify saved bytes.

Automatic decoding accepts at most 2 MiB captured input, caps every decoded stage at 256 KiB,
and displays a 64 KiB text preview. Zstandard's window is capped at 8 MiB. Raw/Hex views remain
available. HAR export respects encoding headers and does not use display-only magic sniffing.

**Show WebSockets**, in History’s Filters dialog, controls visibility of WebSocket handshakes
and messages. It starts enabled, is saved with the project's filters/presets, and applies to
live traffic, saved pages and filtered exports. Hiding WS leaves capture and storage running.
