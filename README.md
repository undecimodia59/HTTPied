# HTTPied

Native Rust HTTP(S) testing workspace for Windows and Linux. Projects keep captures,
Replay drafts, scope, filters, variables and Intruder results in a local SQLite database.
The desktop uses egui/winit/OpenGL with native accessibility support.

## Start

From a packaged archive, run `./httpied` on Linux or `httpied.exe` on Windows.

To build from source, install stable Rust and a C/C++ build toolchain, then run:

```sh
cargo run --locked --release --features ui
```

Choose **Open project**, **Create project**, or **Temporary project** on the start screen.
Temporary projects are deleted on orderly exit. Chromium/Chrome is installed separately;
**Open browser** launches it with an isolated profile and HTTPied's proxy settings.

The workspace has **Intercept, History, Replay, Intruder, Browser, Target, Variables,
Compare, Results, Project and Settings** tabs. Use **Variables** for key/value pairs and
write `{{key}}` in Replay's destination or request. See the
[workflow guide](docs/WORKFLOW.md) for the full controls and limits.

For HTTPS, enable capture in **Settings**, reopen the project, and explicitly trust its
public `ca.pem` in the test browser or operating system. HTTPied never installs trust or
disables certificate verification. Upstream certificates are verified using WebPKI roots.

## Test locally

With a project open and its listener on the default port:

```sh
python3 scripts/ui_fixture.py
```

Open **History** to inspect the fixture traffic, or send a request to Replay. A headless
HTTP/CONNECT proxy is also available:

```sh
cargo run --locked --release -- --headless --listen 127.0.0.1:8080
```

The native executable is `target/release/httpied` on Linux and
`target\release\httpied.exe` on Windows. Linux needs X11 or Wayland and OpenGL/EGL drivers.
Windows builds use stable Rust's MSVC toolchain and Visual Studio C++ Build Tools.
Loopback is required for the listener; port `0` chooses an available port.

## Validation and packaging

```sh
cargo fmt --check
cargo clippy --locked --all-features --all-targets -- -D warnings
cargo test --locked --all-features
cargo test --locked --release --all-features
cargo check --locked --no-default-features
cargo build --locked --release --features ui
python3 scripts/feature_smoke.py
```

The native smoke requires Linux Xvfb, X11/XTest, D-Bus, xclip and ImageMagick.
[CI](.github/workflows/check.yml) defines Windows/Linux checks and build artifacts.
Use `bash scripts/package-linux.sh` or `./scripts/package-windows.ps1` to create a
local archive and checksum in `dist/`. Packages are unsigned; Windows runtime and
Wayland interaction still require platform testing.

[Implementation and verification](docs/IMPLEMENTATION.md) maps the requested feature
numbers to delivered behavior. [Change report](docs/CHANGE_REPORT.md) records measurements
and remaining protocol limits. The broader [feature plan](docs/FEATURE_PLAN.md) remains
the reference for later extensions.
