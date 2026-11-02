# Native UI validation

Build the normal and diagnostic desktop binaries, then run native checks:

```sh
cargo build --locked --release --all-features
python3 scripts/feature_smoke.py --screenshots-dir /tmp/httpied-glow-flow
python3 scripts/feature_smoke.py --layout-only --screenshots-dir /tmp/httpied-glow-layout
python3 scripts/trust_smoke.py
```

Linux native smoke requires Xvfb, libX11/libXtst, D-Bus, xclip, NSS certutil and ImageMagick.
It creates a private display, isolated project/trust store and local HTTP fixture. No public
service is contacted and the operator's regular trust store is not modified.

The full workflow checks:

- Keyboard selection (Shift+arrows) and mouse drag selection survive right-click and transfer
  only the selected text to Coder. Base64 decoding produces the actual clipboard result.
- Compare accepts paired messages and shows separate baseline/candidate results; screenshots
  record aligned colored differences and shared scrolling.
- Replay sends actual local requests, displays readable gzip JavaScript, and retains Undo/Redo
  across cursor movement and sends.
- Variables substitute into an executed Replay request; numeric Intruder payloads keep padding,
  attribution and capture IDs in persisted results.
- History's MIME search finds saved captures; Ctrl+R transfers the executed request.
- Connection failures remain in Logs instead of traffic; the drawer and compact layouts render.
- Closing/reopening a saved project recovers a variable-driven Replay request that executes again.

`--layout-only` captures all seven task tabs and all ten Settings sections. `--capture-only`
checks temporary-session startup, real capture and graceful shutdown. Existing `--review-only`
and `--history-only` flags run the current full workflow. Screenshots default to docs/screenshots;
use `--screenshots-dir` for disposable verification output. Current images use `glow-*.png`.
Older revision/features/workflow images are historical.

The Rust suite covers Unicode context-menu selection, Find focus/Copy, conversion variants and
malformed/binary/oversized data, diff alignment and bounds, compact layouts, project recovery,
TLS verification and early EOF diagnostics, HTTP/2, WebSockets, framing and cancellation.
The abrupt-WebSocket fixture checks that target, direction and failure remain visible.

`trust_smoke.py` additionally requires Chromium and OpenSSL. It verifies a local HTTPS fixture
using an isolated NSS store, with the Chromium sandbox and certificate verification enabled.

Windows, Wayland/IME, hardware OpenGL, screen-reader operation and extended live CTF sessions
still require platform/operator validation. See [UX redesign notes](UX_REDESIGN.md) for the exact
commands/results and [debug logging](DEBUGGING.md) for destination/phase diagnostics.
