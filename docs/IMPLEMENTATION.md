# Requested features — implemented baseline, 2026-11-01

This maps the operator's selected feature-plan numbers to usable behavior.
The workflow guide defines bounds and protocol fidelity; the change report records
verification and platform coverage.

| # | Delivered | Evidence / practical limit |
|---|---|---|
| 1 | Extension include/exclude and MIME History filters | Saved-history filter/page regression; path/query separated |
| 2 | Settings → Scope include/exclude, domain/subdomain/IP/scheme/port/path rules | Subdomains default on; live saved-history refresh; display and execution policies separate |
| 3 | Central versioned Settings | Validated project state; network changes apply on reopen |
| 4 | Column enable/disable/reorder/resize, independent Host/Endpoint/Destination and measured Duration/TTFB/Held | Global saved-history sorting; indexed URL-part migration; saved layouts/widths |
| 5 | Human-readable decimal/binary sizes | Numeric byte sorting and unit-promotion tests |
| 6 | Optional CONNECT/tunnel display | Hidden by default; toggle/preset state persists |
| 7 | Named presets and combined filters | Extension/MIME/method/status/text/minimums/scope combine |
| 8 | Open/create/temporary startup, SQLite captures and Replay recovery | Native saved-project close/reopen/send test; temporary cleanup test |
| 9 | Always-on desktop HTTPS capture/edit, persistent project CA, automatic current-user trust | Verified upstream; isolated NSS idempotence and real Chromium HTTPS trust smoke |
| 12 | Default Auto decoding, line numbers, message search, raw/pretty/hex/decoded/parameter views | Gzip JS over verified HTTPS regression; JSON/XML; bounded gzip/deflate/Brotli/Zstandard/chunk decoding, stacked codecs, charsets and binary fallback |
| 13 | Named/grouped/duplicated/reordered Replay sessions and recovery | Saved timeout overrides; explicit no-follow redirect policy; text-only undo |
| 14 | Text/hex comparison and ignored volatile headers | Bounded LCS; captures/results share History inspection workflow |
| 15 | Durable results below Intruder editors, search/filters/sort and CSV export | Sorting covers the full saved run; run/index/payload/status/bytes/timing/error/match/capture links persist |
| 16 | Pause/resume, cancel/F9, concurrency, rates, retries/backoff, scope, matcher stop | Active-socket cancellation under full result queue is regression-tested |
| 17 | 1–4 lazy sets, Sniper/Pitchfork/Cluster bomb and transforms | Numeric/wordlist/charset/seeded random; overflow/count validation |
| 18 | Conditional request/response interception and ordered rewrites | Before manual edits; original/sent bytes separate; 128 KiB response buffer |
| 20 | HTTP/2 streams and bidirectional WebSocket inspection | Verified TLS/ALPN; stream IDs; reassembled messages/direction/close state |
| 21 | Raw/cURL/HAR/JSONL imports and redacted capture/result exports | Atomic bounded import; all/filter/selected export; no shell execution |
| 22 | Settings → Variables key/value table with {{key}} | One expansion, masked preview, missing/duplicate-key rejection |
| 23 | Six task tabs, uniform Settings subsections, separate History filter/column dialogs, resizable editors, shortcuts | Native X11 input/clipboard smoke; Windows/Wayland/IME/screen-reader checks remain |
| 24 | Bounded caches, paged bodies, measured storage/network/native behavior, package scripts/CI | 100k saved rows, million-item lazy planning, large-body tests; measured report linked below |

All selected numbers have a working baseline. This does not mean every possible extension
in the original proposal has been implemented: there is no automatic redirect following,
raw HPACK/frame recorder, WebSocket editing, external secret manager, signed installer or
cross-platform certification. History complex filters currently scan metadata on the
storage worker; the ordinary page path loads only 128 metadata records.

See [workflow](WORKFLOW.md), [change report](CHANGE_REPORT.md) and
[native testing](UI_TESTING.md). Features 10/11/19 were not selected as standalone work;
remaining HTTP/1 request framing and platform/browser work is explicitly documented.

Application diagnostics use a bottom Logs drawer with a separate additive SQLite table;
legacy Error metadata is copied once and excluded from History/export queries. Traffic and
log retention/clearing remain independent. See ADR-030 and the workflow guide for bounds.
