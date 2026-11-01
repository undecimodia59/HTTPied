# Native UI validation

Build and exercise the current project workflow:

```sh
cargo build --locked --release --features ui
python3 scripts/feature_smoke.py
```

Requires Linux Xvfb, libX11/libXtst, D-Bus, xclip and ImageMagick. The smoke creates an
isolated display and a local HTTP fixture. It verifies saved project creation, actual
Replay sends after Undo/Redo and cursor movement, Variables substitution, padded numeric
Intruder requests, durable attributed results, orderly shutdown, project reopening and
sending a recovered Replay draft. No public service is contacted.

`--capture-only` checks temporary project creation/capture/shutdown.
`--layout-only` captures each workspace tab. Current images use `features-*.png` and
`project-start.png`. Older `revision-*` / `workflow-*` images are historical.
`revision_smoke.py` and `ui_smoke.py` forward to this current harness.

The smoke reports first native-window detection, proxy readiness including automated form
interactions, and idle process RSS. These are Xvfb/software-OpenGL measurements on one
machine, not cold-launch or Windows/Wayland certification.

Network/storage tests separately cover TLS trust/expiry/hostname verification, HTTP/2
streaming beyond the capture cap, bidirectional WebSocket capture, 10 MiB HTTP streaming,
interception/rewrite fidelity, import rollback, paging, scope matching, expansion limits,
attack generation and cancellation while results are backpressured.

Manual platform checks still needed: Windows file picker/browser cleanup, hardware OpenGL,
Wayland clipboard/IME, screen-reader operation, long-running mixed-protocol workloads.
