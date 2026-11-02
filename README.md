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

The workspace has **Intercept, History, Replay, Intruder, Compare and Settings** tabs.
Scope, Variables, Browser and Project are Settings subsections. Intruder results appear
below its request/response editors. Use **Settings → Variables** for key/value pairs and
write `{{key}}` in Replay's destination or request. See the
[workflow guide](docs/WORKFLOW.md) for the full controls and limits.

History offers independent Host, Endpoint and Destination columns. Drag header edges to
resize; widths are saved with the project. Auto response views decode compressed content
with gzip, deflate, Brotli or Zstandard, including stacked encodings and declared text charsets.
Readable text displays automatically; binary/invalid content falls back to captured bytes.
Raw/Hex retain original bytes. Protocol shows HTTP/2; enable Stream ID to inspect stream numbers.

Application errors and warnings appear under **Logs** at the bottom of the window, with search,
severity filters and repeat counts. History contains captured traffic, including HTTP 4xx/5xx.

For a verbose diagnostic build, see [debug logging](docs/DEBUGGING.md). The separate
`httpied-debug` executable records DNS/TCP/TLS/HTTP2 phases and dependency traces to files.

HTTPS interception is always enabled in the desktop app. Opening a project automatically
installs its public CA into the current user's Chromium NSS store on Linux or Root store
on Windows. Linux requires NSS `certutil` (`libnss3-tools` on Debian/Ubuntu,
`mozilla-nss-tools` on openSUSE). **Settings → Certificates** shows setup status and errors.
**Open browser** becomes available after certificate setup and the proxy listener are ready.
Chromium's sandbox and certificate verification remain enabled; upstream certificates are
verified using WebPKI roots. Browsers using a different certificate store need its public
`ca.pem` imported there. Keep the project's `ca.key.pem` private.

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
python3 scripts/trust_smoke.py
```

The native smoke requires Linux Xvfb, X11/XTest, D-Bus, xclip, NSS tools and ImageMagick.
The trust smoke also requires Chromium and OpenSSL. Both use isolated test trust stores.
[CI](.github/workflows/check.yml) defines Windows/Linux checks and build artifacts.
Use `bash scripts/package-linux.sh` or `./scripts/package-windows.ps1` to create a
local archive and checksum in `dist/`. Packages are unsigned; Windows runtime and
Wayland interaction still require platform testing.

[Implementation and verification](docs/IMPLEMENTATION.md) maps the requested feature
numbers to delivered behavior. [Change report](docs/CHANGE_REPORT.md) records measurements
and remaining protocol limits. The broader [feature plan](docs/FEATURE_PLAN.md) remains
the reference for later extensions.


The [2026-11-02 review](docs/REVIEW_2026-11-02.md) records usability fixes and export privacy
changes. History now offers visible Replay/Intruder transfers, saved-row context actions,
and matching page counts. Replay response Find supports Next match and Enter. Redacted
exports also mask URL queries and common API-key headers; redacted cURL masks secret variables.
