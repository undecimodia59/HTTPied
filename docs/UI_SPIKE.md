# Native UI feasibility protocol

Status: egui/eframe capture harness implemented, compile checked and smoke-tested on an
isolated Linux X11 display; permanent selection pending. See UI_TESTING.md for operator steps.
The optional dependency keeps desktop platform code out of the headless build. Eframe
uses winit and the glow/OpenGL backend; no browser engine. No Slint dependency yet.

Run the release spike on the same Windows/Linux machines for each candidate. Record OS,
CPU, RAM, GPU/driver, monitor scaling/refresh, Rust and dependency versions, artifact size,
build flags, idle project state and capture load. Use identical 100k rows, 10 MB read-only
body access, and a 64 KiB editable request. Current egui spike supplies the rows and editor;
the 10 MB body viewer and Slint equivalent remain to implement.

Collect at least 30 process launches after OS reboot/cache conditioning for cold startup,
then 30 warm launches; measure first interactive frame separately from process creation.
Measure idle RSS/private working set for 60 seconds and CPU for 60 seconds. Record p50/p95
scroll, selection and keystroke-to-present latency over at least 100 interactions, including
while the loopback proxy is loaded. Internal update timing is supplementary; present timing
requires platform tracing (PresentMon/ETW on Windows, compositor/frame tracing on Linux).

Acceptance comparison:

| Criterion | Evidence required |
| --- | --- |
| Startup / RAM | <2 s / <200 MiB targets, measured in equivalent release conditions |
| Interaction | p95 <100 ms with 100k rows, scroll/filter/edit and active capture |
| Raw editor | CRLF, unicode, selection, clipboard, undo, 64 KiB edits; 10 MB viewer without redundant copies |
| Native operation | Windows/Linux keyboard, IME, DPI scaling, accessibility and screen readers |
| Distribution | Dynamic dependencies, install size, clean-machine startup, GPU fallback |
| Licensing | Review resolved crate licenses and redistribution terms for both candidates |
| Network independence | UI stalls must not stop sockets until documented event backpressure limits apply |

Select only after saving comparable raw results and documenting tradeoffs in an ADR.
No interactive UI benchmark or Slint comparison was performed during this run.

A supplementary CPU-only experiment is reproducible with:

```sh
cargo run --locked --release --features ui --example ui_layout
```

It samples layout at changing scroll offsets and includes a 35 KiB editable draft, with
30 warmup frames and 300 measured frames. It asserts that fewer than 100 rows are drawn
per layout. Raw results are in `ui-layout-linux.json`; this is not a display/input benchmark.

API references used while implementing: [eframe native runner](https://docs.rs/eframe/0.36.2/eframe/fn.run_native.html),
[Tokio bounded tunnel copying](https://docs.rs/tokio/latest/tokio/io/fn.copy_bidirectional.html).
The compiled local eframe source is authoritative for its newer App::ui interface.
