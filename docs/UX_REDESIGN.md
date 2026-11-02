# CTF workspace UX redesign — 2026-11-02

Recovery commit: `eb170e0` on `improve/usability-security-review`. The redesign is a separate
commit so `git revert <redesign-commit>` restores this recovery point's behavior.

## Research and design decisions

These are documented interaction patterns, not claims that a particular UI caused commercial success.

- [Caido: customizable UI](https://docs.caido.io/app/guides/ui): resizable panes and independent editor sizing support dense inspection workflows.
- [Caido: navigation](https://docs.caido.io/app/guides/navigation) and [shortcuts](https://docs.caido.io/app/guides/shortcuts): predictable tool navigation and keyboard access.
- [Caido: Replay](https://docs.caido.io/app/guides/replay_resending): keep the edit/send/inspect loop direct.
- [Burp: tools](https://portswigger.net/burp/documentation/desktop/tools): move captured data between tools as the investigation develops.
- [Burp: Repeater](https://portswigger.net/burp/documentation/desktop/tools/repeater): independent request sessions and grouped work.
- [Burp: Comparer](https://portswigger.net/burp/documentation/desktop/tools/comparer): paired, highlighted results with synchronized scrolling and difference counts.
- [Burp: Decoder](https://portswigger.net/burp/documentation/desktop/tools/decoder): selected-text transfers, byte-safe Text/Hex views and chained transformations.
- [GNOME: menus](https://developer.gnome.org/hig/patterns/controls/menus.html): group secondary commands by purpose and expected use; avoid deep menus.
- [Windows: navigation](https://learn.microsoft.com/en-us/windows/apps/design/basics/navigation-basics) and [desktop UX checklist](https://learn.microsoft.com/en-us/windows/win32/uxguide/top-violations): consistency, clear paths, useful defaults, and command hierarchy.

Apply these patterns in the existing native egui app. Keep the editor and real captured data central.
Use one prominent execution action per tool. Move infrequent operations to named menus; do not
remove scope, cancellation, fidelity controls or protocol verification to reduce visible controls.

## Review inventory

| Surface | Problems | Redesign acceptance |
| --- | --- | --- |
| Workspace | Repeated execution controls and status; off-by-default Intercept is the landing view | History-first navigation, one global browser action, active-task status, keyboard tool navigation |
| History | Two filter/search rows, fixed list height, five competing message tabs, repetitive transfer actions | Unified search/filter toolbar, resizable list, simultaneous request/response inspection, compact secondary actions |
| Intercept | Forward-original duplicates Forward even without edits; inert page when disabled | Contextual forwarding, visible hold deadline, clear waiting/pass-through state and one interception switch |
| Replay | Session operations, rename, destination, cURL variants and resolved preview occupy many rows | Compact session strip and destination/send bar; session/request menus; large request/response editors |
| Intruder | Many source tabs and options crowd editors; fixed results consume space | Source chooser, expandable configuration, bounded run controls and resizable results |
| Compare | Two tiny editors plus separate unstyled diff; unclear source and stale/manual result | Full-height paired inputs/results, color-coded aligned diff, shared scrolling, next/previous change, source actions in message editors |
| Coder | New feature requires actual mouse/keyboard selection verification and clean binary handling | Selection context action, six formats, exact bytes/hex display, bounded chaining, two clear panes |
| Settings | Repeated headings, flat section list, oversized fields | Grouped sidebar, consistent section headings, adaptive fields; advanced controls remain available |
| Logs | Repeated close/clear actions and crowded severity switches | Search-first toolbar, secondary actions grouped, contextual failures retained |
| Project start | Unclear saved/temporary distinction and verbose layout | Clear open/create/session actions, useful directory guidance |

## Delivered behavior

- History is the landing view. One search/filter bar replaces the stacked filter/search rows;
  advanced filter counts are visible and secondary column/reload/clear actions live under View.
  Executed requests and responses are displayed together. Original/sent variants and the scratch
  draft remain available through More. The list and editor separator are resizable; pane widths
  adjust with the window. Captured-byte truncation remains explicit.
- Intercept retains one necessary on/off control. Forward is primary; forwarding without edits
  appears only after editing. Hold deadlines and a useful pass-through/waiting state remain visible.
- Replay has a compact session strip, destination/send bar, Session menu and Request actions.
  Duplicate/move/close, rename, cURL variants, Content-Length repair and resolved previews remain
  available without occupying separate command rows. Stop appears during an active run. Other
  tools can still be prepared while a job runs; only one job executes at once.
- Intruder uses one payload-source chooser and expandable configuration. Results resize separately
  and grow to a useful minimum height after a run; source generation, cancellation, matching and
  attribution retain their existing semantics.
- Compare has full-height paired inputs and an aligned, colored result view rather than three
  small competing panes. The views scroll together, line numbers are not duplicated, changes
  have a count and Previous/Next navigation, and empty-side comparisons are supported. Volatile
  header exclusion and hex-body comparison remain under Options.
- Coder receives exact Unicode selections from editable/read-only message editors. It supports
  Base64, Base64url, strict URL/form percent handling, HTML5 named/numeric entities and hex.
  Decoded binary is retained as bytes; Copy/Use as input operate on the complete result, with
  explicit input/output and preview bounds. Coder is session-only.
- Settings uses grouped navigation. Variable enable/secret/remove controls move to row Options;
  invalid/duplicate configured shortcuts have visible feedback. Imports and supported export
  formats are distinguished, and redundant manual Save is replaced by autosave status.
- Browser start/stop has one global action surface; Browser settings configure it. Logs groups
  severity and maintenance actions in menus instead of repeating close/clear controls. The start
  screen distinguishes saved projects from a temporary session.

## Verification

Environment: Linux 7.3.0-1-default, Rust/Cargo 1.99.0. Native checks use Xvfb/software OpenGL,
local fixtures, isolated projects and isolated NSS trust storage. Native first-window/RSS output
is a harness observation, not a general startup/performance certification.

| Command | Result |
| --- | --- |
| `cargo test --locked --all-features` | 112 tests passed (68 library + 44 integration) |
| `cargo test --locked --release --all-features` | 112 tests passed |
| `cargo clippy --locked --all-features --all-targets -- -D warnings` | Passed |
| `cargo check --locked --no-default-features` | Passed |
| `cargo fmt --check` and `git diff --check` | Passed |
| `cargo build --locked --release --all-features` | Passed; normal and diagnostic binaries rebuilt |
| `python3 -m py_compile scripts/feature_smoke.py` | Passed |
| `python3 scripts/feature_smoke.py --screenshots-dir /tmp/httpied-glow-final` | Passed full native CTF workflow and project recovery |
| `python3 scripts/feature_smoke.py --layout-only --screenshots-dir /tmp/httpied-glow-final-layout` | Passed seven tabs / ten Settings sections |
| `python3 scripts/trust_smoke.py` | Passed with real Chromium 156.0.8078.73, isolated NSS trust and verified local HTTPS |

Native socket/display checks use approved outside-sandbox execution. After the last History
geometry adjustment, the 68 library tests/layout checks, Clippy, both executable builds and
full native workflow were repeated successfully; the complete 112-test debug/release runs
also passed as recorded above. Compile/lint failures
encountered during implementation were corrected before the successful final runs above.

Screenshots: [History](screenshots/glow-history.png), [Replay](screenshots/glow-replay-sent.png),
[Intruder results](screenshots/glow-intruder-results.png), [Compare](screenshots/glow-compare-diff.png),
[Coder](screenshots/glow-coder-decoded.png), [selection context menu](screenshots/glow-selection-menu.png),
[Variables](screenshots/glow-variables-filled.png), [Logs](screenshots/glow-logs.png),
and [compact History](screenshots/glow-compact-history.png). All ten Settings sections and all
seven compact task views are saved alongside these using `glow-settings-*` / `glow-compact-*` names.
The screenshot review explicitly checked
1240×860 and 940×700 layouts. The native drag test was strengthened to transfer a token out of a
larger string; this avoids passing merely because the clipboard or original Coder input already
contained the expected value. Simulated drag movement is flushed over multiple frames.

The native workflow covers selection-to-Coder by keyboard and mouse, Base64 output, highlighted
paired Compare, actual Replay sends and Undo/Redo, gzip response display, Variables substitution,
padded numeric Intruder requests and durable result attribution, MIME History search, History
transfer, refusal Logs, compact layouts, saved project reopening and recovered Replay execution.
The separate real Chromium fixture verified local HTTPS trust with no sandbox/certificate bypass.

## Source changes and limits

The redesign changes `src/ui.rs`, `src/ui/workflow.rs`, `src/ui/features.rs`, `src/ui/settings.rs`,
`src/ui/log_view.rs`, `src/ui/coder.rs`, `src/ui/syntax.rs`, and `src/ui/widgets.rs`. New UI modules
`src/ui/history.rs` and `src/ui/compare.rs` isolate the paired inspector and comparison state.
The native smoke, README/workflow/testing guides and this report are updated. No new crate,
project schema or transport-policy changes are introduced by the redesign commit.

The preceding recovery commit also contains the requested Coder core, selected-text foundation
and diagnostic context work, along with all preexisting workspace work. It preserves TLS,
certificate, HTTP framing and WebSocket errors. The user's EOF/reset excerpts establish peer
termination, not a particular cause. Context now identifies the connection, destination and
phase/direction. [Rustls explains](https://docs.rs/rustls/latest/rustls/manual/_03_howto/index.html#unexpected-eof)
why missing close_notify can be tolerable on an idle/framed connection but must not be ignored
unconditionally. Abrupt TLS and WebSocket fixtures retain visible failure diagnostics.

Windows, Wayland/IME, screen readers, hardware OpenGL and long-running public-lab usage remain
operator/platform validation tasks. This is an operator-driven HTTP testing tool with the
existing protocol limits documented in WORKFLOW.md, not full Burp/Caido feature parity. Coder
accepts 128 KiB of bytes and produces at most 1 MiB. Compare remains bounded to 2,048 lines and
256 KiB per side. Next bounded task: exercise one actual CTF lab and validate the platform-specific
clipboard/IME/browser lifecycle with the same capture → Replay → Coder → Compare workflow.

