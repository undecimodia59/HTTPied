# HTTPied

Native Rust HTTP(S) testing workspace for Windows and Linux. Projects keep captures,
Replay and Coder workspaces, scope, filters, variables, and Intruder/Attack results in a local
SQLite database.
The desktop uses egui/winit/OpenGL with native accessibility support.

## Start

From a packaged archive, run `./httpied` on Linux or `httpied.exe` on Windows.

To build from source, install Rust **1.95 or newer** and a C/C++ build toolchain, then run:

```sh
cargo run --locked --release --features ui
```

Choose **Open project**, **Create project**, or **Temporary session** on the start screen.
Temporary projects are deleted on orderly exit. Chromium/Chrome is installed separately;
**Open browser** launches it with an isolated profile and HTTPied's proxy settings.

The workspace has **Intercept, History, Replay, Intruder, Attack, Compare, Coder and Settings** tabs.
Scope, Variables, Browser and Files & storage are grouped under Settings. Intruder results appear
below its request/response editors. Use **Settings → Variables** for key/value pairs and
write `{{key}}` in Replay's destination or request.

For a CTF: capture traffic in **History**, select a request and press **Ctrl+R** to edit it
in Replay, then **Ctrl+Enter** to send. History shows executed requests and responses together.
Session management and less frequent actions are in named menus; message panes and Intruder
results can be resized by dragging their separators.

Replay has a searchable sidebar with named, collapsible groups. Create a group with **+**, move
requests through the Session menu or drag them onto a group, and use Ctrl/Shift to select several
requests. Names, groups, order, drafts and the selected request survive reopening. Replay supports
64 requests; **Send selection to Attack** and the group's context menu hand off request snapshots.
Group execution happens in **Attack**, not in Replay.

**Attack (Ctrl+7)** offers three experiment modes. **Race condition** repeats one request or cycles
through several inputs, with configurable bursts, parallel connections, delay and timeout. Its
HTTP/1.1 synchronization prepares all connections before releasing each request's final byte;
HTTP/2 single-packet sending is not implemented. Client release timings do not measure simultaneous
server processing. Project rate limits apply; synchronized sending reports conflicting limits.
**Access-control** sends a fresh original baseline and variants with alternate credential headers
and optional anonymous checks. Set headers directly or reference project variables. Matching
successful bodies are potential bypasses requiring review; HTTP denial statuses and a configured
denial marker help classify results. Anonymous checks strip configured credential headers,
including Authorization and Cookie; credentials in URLs or bodies require manual changes.
**Sequence** runs requests in order, extracts JSON pointers, response headers, cookies or regex
captures into `{{variables}}`, and optionally carries cookies on the same origin. Expected status
prefixes and required extractors can stop dependent steps. Each cycle starts with fresh runtime
variables and cookies. Experiments support 32 inputs, 4,096 attempts and 64 parallel connections.
Runs do not retry automatically or apply proxy rewrite rules. Saved runs retain their plans,
outcomes, result pages and links to captured messages. Click any result to inspect its exact request
and response; Access-control variants can open Compare against their original response.

Coder includes a **JWT workbench** for local header/claims inspection, expiry hints, explicit
HS256/HS384/HS512 signing and verification, and unsigned `alg=none` test tokens. Selected valid JWTs
open it automatically. Editing claims builds a new token; decoding alone does not verify a signature.
JWT inputs, output and signing keys are session-only.

Select text with the mouse or **Shift+arrows**, right-click, and choose **Send selection to Coder**.
Coder (**Ctrl+5**) supports Base64, Base64url, URL percent, form, HTML entities and hex. Binary
results stay as exact bytes with a hex view; **Use as input** chains a result into another operation.
Encoder input, input format, codec, last encode/decode choice and result view are saved with
the project. In a message editor, **Set Compare baseline** followed by **Compare with baseline**
opens the paired, highlighted comparison (**Ctrl+6**) with synchronized scrolling.
You can also paste two messages directly into Compare.

Request and response context menus offer **Add to scope → Host only / Host + subdomains**.
For subdomains, **Site domain only / Site domain + subdomains** also let you add the registrable
site domain, for example `domain.com` from `sub.domain.com`, or `example.co.uk` from
`shop.example.co.uk`. The menu shows both target names before you choose. Site-domain lookup uses
a bundled Public Suffix List, including private suffixes such as `github.io`, without network
requests. Equivalent site actions are omitted when the host already is the site domain; IPs and
single-label local names keep the host actions. Unknown suffixes follow the list's default rule,
so CTF names such as `api.lab.htb` resolve to `lab.htb`. Subdomain options include the named host
and its children. Existing inclusions are reused or expanded; explicit exclusions and restricted
rules remain intact.

Settings is the last workspace tab. Enable **Settings → Appearance → Wrap long lines by default**
to wrap all message editors; individual panes also have a Wrap control/context-menu option.
Right-click a request editor to send it to Replay, Intruder or Attack, preserving its destination.
Click anywhere on an Intruder result row to inspect that payload's captured request and response;
**Attack template** returns to the original template. Failed attempts show their recorded error.

History offers independent Host, Endpoint and Destination columns. Drag header edges to
resize; widths are saved with the project. Auto response views decode compressed content
with gzip, deflate, Brotli or Zstandard, including stacked encodings and declared text charsets.
Readable text displays automatically; binary/invalid content falls back to captured bytes.
Raw/Hex retain original bytes. Protocol shows HTTP/2; enable Stream ID to inspect stream numbers.

Application errors and warnings appear under **Logs** at the bottom of the window, with search,
severity filters and repeat counts. History contains captured traffic, including HTTP 4xx/5xx.

Build both executables with `cargo build --locked --release --all-features`. The separate
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

## Debian 13 (Trixie) / Secbian builds

Trixie's packaged Rust 1.85 is too old for the UI dependencies. Use `rustup` to
install a newer compiler while keeping Debian's system libraries:

```sh
sudo apt update
sudo apt install rustup ca-certificates build-essential pkg-config \
  libx11-dev libxi-dev libgl1-mesa-dev libxkbcommon-dev libwayland-dev libnss3-tools
rustup toolchain install 1.95.0 --profile minimal
rustup run 1.95.0 cargo build --locked --release --features ui --bin httpied
./target/release/httpied
```

Debian's `rustup` package replaces the distribution's `cargo`/`rustc` packages;
APT will show the proposed replacements. If you already installed rustup from
[rust-lang.org](https://rust-lang.org/tools/install/), keep that installation and
omit `rustup` from the APT command.

Build on Trixie (or inside a Trixie container) for Secbian. Binaries built on a
newer distribution can require newer GLIBC symbols and fail on Trixie's GLIBC
2.41. Installing a newer Rust compiler on Trixie does not upgrade its GLIBC.
CI's Linux archive is built with Rust 1.95.0 inside a Trixie container. Local
packaging uses the host's libraries, so its compatibility depends on the build
host. Keep the existing dependency lockfile and use `--locked`.

## Validation and packaging

```sh
cargo fmt --check
cargo clippy --locked --all-features --all-targets -- -D warnings
cargo test --locked --all-features
cargo test --locked --release --all-features
cargo check --locked --no-default-features
cargo build --locked --release --features ui
python3 scripts/feature_smoke.py
python3 scripts/feature_smoke.py --attack-only
python3 scripts/trust_smoke.py
```

The native smoke requires Linux Xvfb, X11/XTest, D-Bus, xclip, NSS tools and ImageMagick.
The trust smoke also requires Chromium and OpenSSL. Both use isolated test trust stores.
[CI](.github/workflows/check.yml) defines Windows/Linux checks and build artifacts.
Use `bash scripts/package-linux.sh` or `./scripts/package-windows.ps1` to create a
local archive and checksum in `dist/`. Packages are unsigned; Windows runtime and
Wayland interaction still require platform testing.

Documentation and screenshot directories are local ignored artifacts and are not included in packages.
