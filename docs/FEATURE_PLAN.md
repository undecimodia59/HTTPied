# HTTPied feature plan

Recorded 2026-10-31. This is a proposed backlog, not a list of completed features or a
commitment to implement everything at once. Items 1–6 capture the operator's requests;
the remaining items are recommendations based on the current implementation.

**Suggested first delivery:** 5 and 6 as small improvements, then 1 and 2, then 3 and 4.
Build shared configuration/persistence support while implementing those controls so their
state can survive restart. General HTTPS support (9) and persistent history (8) are the
highest technical priorities for everyday browser testing.

1. **Filter History by extension and content type — requested.**
   Add include/exclude controls for CSS, JavaScript, source maps, images, fonts, media,
   HTML, JSON/XML, and other/unknown content. Allow explicit extensions such as `.css`,
   `.js`, `.map`, `.svg`, and `.woff2`, including extensionless requests as a selectable
   category. Extension matching uses the URL path, ignoring query parameters and case;
   `/app.JS?v=12` is JavaScript. Content-type groups use response MIME types where useful,
   so extensionless CSS is still identifiable. Keep extension and MIME criteria explicit
   when they disagree. Filters must not delete captures or affect forwarding.

2. **Add a Target tab with scope rules — requested.**
   Store named targets with domain/subdomain include and exclude rules. Support exact
   hosts, an explicit “include subdomains” option, and optional scheme, port and path
   restrictions. Exclusions take precedence. Match DNS labels correctly: including
   `example.com` must never match `notexample.com`. Normalize case and internationalized
   names; support explicit IP targets as well. The wildcard `*.example.com` means
   subdomains, with the apex included separately or through the subdomains option.
   Add “In scope only” to History, backed by the selected target. Without configured
   include rules, leave that option disabled with a link to Target instead of hiding
   everything unexpectedly. Display filtering and send/forward enforcement are separate
   settings: adding a display rule must not silently block traffic. Offer explicit scope
   enforcement for Replay/Intruder later, with the resolved destination shown before send.

3. **Centralize configuration in Settings — requested.**
   Add a Settings tab for proxy listener, timeouts, capture/retention limits, interception
   behavior, browser executable/profile, editor preferences, shortcuts and History defaults.
   Separate global preferences from project/target settings. Show effective values and
   identify CLI overrides. Validate changes before applying; keep the last working
   configuration if validation fails. Clearly mark settings that need a listener or browser
   restart. Persist preferences using a versioned format and provide Reset defaults.
   Add controls only for implemented capabilities, rather than disabled placeholder pages.

4. **Make History columns configurable and add request timings — requested.**
   Provide a column chooser on the header context menu, plus reorder, resize and saved
   layouts. Candidate columns: ID, start time, method, host, path/query, status, MIME type,
   duration, time to first byte, request size, response size, source, protocol and port.
   Make response size optional as requested. Instrument the proxy first: current Replay
   job timing alone cannot supply accurate timings for all History rows. Define “duration”
   consistently; show upstream execution time separately from interception hold time.
   Use monotonic measurements, show unavailable values as `—`, and keep raw values for
   numeric sorting. Long values should elide with tooltips rather than expand the table.

5. **Use readable byte units — requested; small first change.**
   Format sizes as `842 B`, `1.2 KB`, `8.4 MB`, `1.1 GB`, etc., with consistent precision
   and right alignment. Recommend decimal units by default (1 KB = 1000 B), with an optional
   binary preference using the correct KiB/MiB/GiB labels. Promote units when rounding would
   otherwise produce a long `1000 KB` value. A tooltip retains the exact byte count.
   Sort by the underlying integer, never by the formatted string. Keep total transferred
   response size distinct from retained capture size and decoded body size.

6. **Make CONNECT/tunnel rows optional — requested; small first change.**
   Add “Show CONNECT/tunnels” to History filters and its saved defaults; recommend OFF
   for normal HTTP inspection. This hides transport-only rows, not all HTTP traffic
   merely because it uses TCP. Future decrypted HTTPS child requests must stay visible
   when their parent CONNECT row is hidden. Keep connection failures discoverable through
   the error filter/log even when successful tunnels are hidden. Forwarding and capture
   behavior must not change when this display option changes.

7. **Save filter presets and make combined filters predictable.**
   Supply editable presets such as “In-scope traffic”, “API responses”, “Errors” and
   “Hide static assets”. Combine different filter dimensions with AND, allow multiple
   values within a dimension, and give exclusions priority. Show the active filters,
   visible/retained counts, Clear filters, and a useful no-results state. Persist per-project
   choices. Extend the existing text search with method, status class, MIME, source, size
   and duration conditions; report invalid input rather than silently guessing.

8. **Persist projects and History — high priority.**
   Add project save/open, SQLite metadata and bounded on-disk body storage with lazy loading.
   The current 128-event in-memory history is too limited for sustained work and large
   Intruder runs. Make retention, disk quotas, purge and crash recovery explicit; never grow
   RAM in proportion to all captured bodies. Save target rules, filters, column layouts and
   Replay requests with the project. Do not start stored jobs automatically on reopen.

9. **Complete general HTTPS interception — high priority.**
   Replace the fixed-host TLS experiment with an explicit local CA lifecycle, correctly
   named per-host certificates, upstream verification and decrypted HTTP capture/edit.
   Provide a clear browser trust workflow and visible TLS diagnostics. Installing OS trust
   must remain an explicit operator action; do not bypass certificate verification.
   Ensure scope and capture settings work on decrypted requests as well as plain HTTP.

10. **Finish browser-compatible HTTP request handling.**
    Add request chunking, Expect handling, keepalive and request streaming where editing is
    unnecessary. Keep backpressure, cancellation and request-size policies explicit. Record
    transformations and test ambiguous framing, partial reads and upstream disconnects.
    Preserve the response streaming fixes already implemented. Add bounded gzip/deflate/
    Brotli decoding as inspector views, with expansion limits and original bytes preserved.

11. **Provide a useful diagnostic log.**
    Structure failures by request ID, target, phase, elapsed time and cause. Separate normal
    cancellation, warnings and genuine errors; group repeated failures with counts and
    timestamps without losing access to individual events. Link a log entry to its capture
    and effective timeout/settings. Add Copy diagnostic details with credential redaction.
    Never claim an unavailable remote service has been repaired by hiding its refusal.

12. **Improve request/response inspection.**
    Add line numbers, in-message search, raw/pretty/hex views, JSON/XML formatting, cookie
    and parameter views, and decoded content. Mark truncated and decoded views prominently.
    Formatting is a view operation until the operator explicitly applies an edit. Continue
    highlighting HTML as text; do not execute page scripts or embed a webview in the app.

13. **Improve Replay session management.**
    Add request names, duplicate/reorder tabs, grouping, dirty indicators and reliable
    recovery of unsaved edits. Preserve each session's undo/redo and selection state.
    Keep destination and request source visible; offer explicit timeout overrides and
    redirect policy. Avoid automatic retries of side-effecting requests. Store Replay
    sessions with projects once persistence exists.

14. **Add response comparison.**
    Select a baseline and compare status, headers, bodies, lengths and timings against
    another response. Offer text/hex differences and optional ignored volatile fields such
    as Date/request IDs. Keep ignored-field rules in the comparison view rather than
    modifying captures. Make comparisons useful from both History and Intruder.

15. **Give Intruder a durable results table.**
    Record payload attribution, status, length, duration, errors and selected matchers for
    every result, with filtering, sorting, baseline comparison and export. Use paged/disk
    storage for large runs rather than keeping all response bodies in RAM. Expose matches
    and anomalies as evidence, not automatic vulnerability verdicts.

16. **Add execution controls and scope enforcement.**
    Support pause/resume, stop, bounded concurrency, per-host/global rates, delays, retries
    and configurable 429/503 backoff. Keep one clear emergency stop. Apply an explicitly
    enabled target policy to every generated request and followed redirect. Show planned
    request count and destinations before a run; avoid hidden fan-out or automatic restart.

17. **Extend payload generation.**
    Add multiple insertion points, Sniper/Pitchfork/Cluster bomb modes, character sets,
    URL/Base64 encoding, prefixes/suffixes and seeded reproducible random values. Estimate
    run sizes without expanding products, and generate payloads lazily. Preserve the original
    template and attribute each executed request to its payload tuple. Validate overflow and
    excessive run sizes before sending.

18. **Add conditional interception and ordered rewrite rules.**
    Pause only selected hosts, paths, methods, statuses or MIME types. Add response
    interception and deterministic scoped match/replace rules. Make rule order and whether
    a rule runs before or after manual edits visible. Explain body-buffering limits and
    hold-expiry behavior through Settings; never silently alter originals.

19. **Improve browser lifecycle and target integration.**
    Open the selected target directly, show the active listener/profile/trust state, and
    make browser startup errors actionable. Handle listener changes through an explicit
    browser restart/reconfiguration flow. Verify clean process/profile cleanup on Windows,
    X11 and Wayland. Keep the isolated test profile separate from the operator's usual one.

20. **Add HTTP/2 and WebSocket inspection.**
    Show protocol-specific stream/frame data, direction and close/error state. Preserve
    bounded flow control and cancellation. Clearly distinguish HTTP/2's parsed header view
    from HTTP/1 raw text, and WebSocket messages from individual frames. Add editing only
    after capture/re-encoding semantics are tested.

21. **Add practical import/export.**
    Support copy-as-cURL, import cURL/raw requests, HAR where representable, and selected
    History/Intruder export. Include scope/filter choices where appropriate. Provide explicit
    credential redaction, versioned project formats and safe import validation; disclose
    fidelity loss. Imported projects must not auto-run requests or scripts.

22. **Add local environments and reusable values.**
    Allow named variables for base URLs, tokens and repeated headers, with an explicit
    per-request preview of resolved values. Later add bounded extractors from prior
    responses. Keep secrets local, mask them where useful, and avoid leaking target-specific
    authorization headers to another host or through redirects.

23. **Refine native interaction and accessibility.**
    Add resizable request/response splits, multi-select History actions, configurable
    shortcuts, reliable focus, font scaling, contrast preferences and remembered layouts.
    Test IME, clipboard, keyboard-only operation and screen-reader access on Windows/Linux.
    Keep actions in the requested tabs and relevant context menus; avoid decorative
    dashboards, redundant badges or controls for unimplemented capabilities.

24. **Measure and optimize real workloads.**
    Profile long browser sessions and large projects: memory, handles, CPU, first-byte
    latency, UI input latency and startup. Page History metadata, fetch bodies on demand,
    cache bounded syntax layouts, and coalesce UI updates instead of repainting unchanged
    screens continuously. Add Windows/Linux release builds, packaging and regression gates.
    Publish the environment and measured results; do not infer current performance from
    the earlier synthetic spike.

## Delivery boundaries

- This document records proposals only. None of the six new requested controls is marked
  implemented by creating this plan.
- Display filters are reversible and affect visibility. Capture retention, interception
  rules and outbound scope enforcement are independently named policies.
- Persist raw values alongside display formatting, timings and view transformations.
- Deliver small slices with local protocol regressions and native interaction checks;
  do not start all roadmap items in parallel.

The recommended next bounded slice is **human-readable sizes plus CONNECT visibility**
(5–6), followed by the shared filter model and Target rules (1–2). Settings and configurable
columns (3–4) should reuse that model rather than introduce another configuration system.
