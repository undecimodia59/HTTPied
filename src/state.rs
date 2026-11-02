//! Versioned project settings and pure filtering/scope/variable policy.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
/// Hostname and encoded path/query for independent History columns. Fragments are not sent.
pub fn destination_parts(target: &str) -> (String, String) {
    if let Ok(url) = url::Url::parse(target)
        && let Some(host) = url.host_str()
    {
        return (
            host.to_owned(),
            url[url::Position::BeforePath..url::Position::AfterQuery].to_owned(),
        );
    }
    // CONNECT targets contain an authority rather than a URL.
    if !target.contains(['/', ' ', '\n', '\r'])
        && let Ok(url) = url::Url::parse(&format!("http://{target}"))
        && let Some(host) = url.host_str()
    {
        return (host.to_owned(), String::new());
    }
    (String::new(), String::new())
}
#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct Settings {
    pub listen: String,
    pub http_timeout: u64,
    pub idle_timeout: u64,
    pub connect_timeout: u64,
    pub capture_mib: usize,
    pub retain_rows: usize,
    pub retain_mib: usize,
    pub disk_quota_mib: u64,
    pub browser: String,
    pub browser_url: String,
    pub font_size: f32,
    pub binary_units: bool,
    pub enforce_scope: bool,
    pub concurrency: usize,
    pub delay_ms: u64,
    pub retries: usize,
    pub global_rps: u32,
    pub host_rps: u32,
    pub backoff_ms: u64,
    pub intercept_responses: bool,
    pub request_ratio: f32,
    pub shortcuts: BTreeMap<String, String>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            listen: "127.0.0.1:8080".into(),
            http_timeout: 120,
            idle_timeout: 10,
            connect_timeout: 5,
            capture_mib: 1,
            retain_rows: 128,
            retain_mib: 16,
            disk_quota_mib: 2048,
            browser: String::new(),
            browser_url: "about:blank".into(),
            font_size: 13.0,
            binary_units: false,
            enforce_scope: false,
            concurrency: 1,
            delay_ms: 250,
            retries: 0,
            global_rps: 0,
            host_rps: 0,
            backoff_ms: 1000,
            intercept_responses: false,
            request_ratio: 0.5,
            shortcuts: [
                ("History", "Ctrl+1"),
                ("Replay", "Ctrl+2"),
                ("Intruder", "Ctrl+3"),
                ("Intercept", "Ctrl+4"),
                ("Settings", "Ctrl+Comma"),
                ("Send", "Ctrl+Enter"),
                ("Stop", "F9"),
            ]
            .into_iter()
            .map(|(a, b)| (a.into(), b.into()))
            .collect(),
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        let address: std::net::SocketAddr = self
            .listen
            .parse()
            .map_err(|_| "Invalid listener address")?;
        if !address.ip().is_loopback() {
            return Err("Listener must be loopback".into());
        }
        if [self.http_timeout, self.idle_timeout, self.connect_timeout]
            .iter()
            .any(|v| !(1..=86400).contains(v))
        {
            return Err("Timeouts must be 1–86400 seconds".into());
        }
        if !(1..=16).contains(&self.capture_mib)
            || !(16..=1024).contains(&self.retain_mib)
            || !(16..=4096).contains(&self.retain_rows)
            || !(64..=1048576).contains(&self.disk_quota_mib)
        {
            return Err("Invalid capture/retention/disk limits".into());
        }
        if !(1..=16).contains(&self.concurrency)
            || self.retries > 3
            || self.delay_ms > 60000
            || !(9.0..=28.0).contains(&self.font_size)
            || !(0.15..=0.85).contains(&self.request_ratio)
        {
            return Err("Invalid execution/editor settings".into());
        }
        Ok(())
    }
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ScopeRule {
    pub host: String,
    pub exclude: bool,
    pub subdomains: bool,
    pub scheme: String,
    pub port: String,
    pub path: String,
}
impl Default for ScopeRule {
    fn default() -> Self {
        Self {
            host: String::new(),
            exclude: false,
            subdomains: true,
            scheme: String::new(),
            port: String::new(),
            path: String::new(),
        }
    }
}
impl ScopeRule {
    pub fn matches(&self, url: &url::Url) -> bool {
        let pattern = self.host.trim().trim_end_matches('.');
        let wild = pattern.starts_with("*.");
        let pattern = pattern.trim_start_matches("*.");
        let Ok(normalized) = url::Host::parse(pattern) else {
            return false;
        };
        let normalized = normalized.to_string().to_lowercase();
        let host = url
            .host_str()
            .unwrap_or("")
            .trim_matches(['[', ']'])
            .trim_end_matches('.')
            .to_lowercase();
        let normalized = normalized.trim_matches(['[', ']']);
        let exact = host == normalized;
        let child = host
            .strip_suffix(normalized)
            .is_some_and(|prefix| prefix.ends_with('.') && prefix.len() > 1);
        (if wild {
            child
        } else {
            exact || (self.subdomains && child)
        }) && (self.scheme.is_empty() || self.scheme.eq_ignore_ascii_case(url.scheme()))
            && (self.port.is_empty()
                || self.port.parse::<u16>().ok() == url.port_or_known_default())
            && (self.path.is_empty() || url.path().starts_with(&self.path))
    }
}
pub fn in_scope(rules: &[ScopeRule], target: &str) -> bool {
    let Ok(url) = url::Url::parse(target) else {
        return false;
    };
    !rules.iter().any(|r| r.exclude && r.matches(&url))
        && rules.iter().any(|r| !r.exclude && r.matches(&url))
}
#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct HistoryFilter {
    pub text: String,
    pub extensions: String,
    pub excluded_extensions: String,
    pub mime: String,
    pub in_scope: bool,
    pub show_ws: bool,
    pub show_tunnels: bool,
    pub errors_only: bool,
    pub method: String,
    pub status: String,
    pub min_ms: f64,
    pub min_bytes: u64,
    pub sort_by: String,
    pub descending: bool,
}
impl Default for HistoryFilter {
    fn default() -> Self {
        Self {
            text: String::new(),
            extensions: String::new(),
            excluded_extensions: String::new(),
            mime: String::new(),
            in_scope: false,
            show_ws: true,
            show_tunnels: false,
            errors_only: false,
            method: String::new(),
            status: String::new(),
            min_ms: 0.0,
            min_bytes: 0,
            sort_by: String::new(),
            descending: false,
        }
    }
}
impl HistoryFilter {
    fn extension_allows(&self, target: &str) -> bool {
        let path = url::Url::parse(target)
            .ok()
            .map(|url| url.path().to_owned())
            .unwrap_or_default();
        let name = path.rsplit('/').next().unwrap_or("");
        let decoded = percent_encoding::percent_decode_str(name).decode_utf8_lossy();
        let extension = decoded
            .rsplit_once('.')
            .map(|(_, extension)| extension)
            .unwrap_or("(none)");
        let contains = |list: &str| {
            list.split([',', ';', ' ']).any(|item| {
                item.trim()
                    .trim_start_matches('.')
                    .eq_ignore_ascii_case(extension)
            })
        };
        let has_include = self
            .extensions
            .split([',', ';', ' '])
            .any(|item| !item.trim().is_empty());
        (!has_include || contains(&self.extensions)) && !contains(&self.excluded_extensions)
    }
    pub fn matches(&self, event: &crate::model::Event, scope: &[ScopeRule]) -> bool {
        use crate::model::Event;
        match event {
            Event::Exchange(e) => {
                if !self.show_ws && e.protocol == "WebSocket handshake" {
                    return false;
                }
                if self.errors_only && e.response_status < 400 {
                    return false;
                }
                if self.in_scope && !in_scope(scope, &e.parsed.target) {
                    return false;
                }
                if !self.method.is_empty() && !self.method.eq_ignore_ascii_case(&e.parsed.method) {
                    return false;
                }
                if !self.status.is_empty()
                    && !e.response_status.to_string().starts_with(&self.status)
                {
                    return false;
                }
                if e.elapsed_ms < self.min_ms || e.response_total_bytes < self.min_bytes {
                    return false;
                }
                if !self.extension_allows(&e.parsed.target) {
                    return false;
                }
                if !self.mime.is_empty()
                    && !e
                        .response_mime
                        .to_lowercase()
                        .contains(&self.mime.to_lowercase())
                {
                    return false;
                }
                self.text.is_empty()
                    || format!(
                        "{} {} {} {}",
                        e.parsed.method, e.parsed.target, e.response_status, e.response_mime
                    )
                    .to_lowercase()
                    .contains(&self.text.to_lowercase())
            }
            Event::Tunnel { authority, .. } | Event::InterceptedTls { authority, .. } => {
                self.show_tunnels
                    && !self.errors_only
                    && (!self.in_scope || in_scope(scope, &format!("https://{authority}")))
                    && (self.text.is_empty()
                        || authority.to_lowercase().contains(&self.text.to_lowercase()))
            }
            Event::WebSocket {
                target,
                kind,
                total,
                ..
            } => {
                self.show_ws
                    && !self.errors_only
                    && (self.method.is_empty() || self.method.eq_ignore_ascii_case("WS"))
                    && self.status.is_empty()
                    && self.mime.is_empty()
                    && self.min_ms == 0.0
                    && *total as u64 >= self.min_bytes
                    && self.extension_allows(target)
                    && (!self.in_scope || in_scope(scope, target))
                    && (self.text.is_empty()
                        || format!("{target} {kind}")
                            .to_lowercase()
                            .contains(&self.text.to_lowercase()))
            }
            Event::Error(_) => false,
            Event::Listening(_) => false,
        }
    }
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Preset {
    pub name: String,
    pub filter: HistoryFilter,
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Variable {
    pub key: String,
    pub value: String,
    pub secret: bool,
    pub enabled: bool,
}
pub fn resolve(text: &str, variables: &[Variable]) -> Result<String, String> {
    let mut values = BTreeMap::new();
    for var in variables.iter().filter(|v| v.enabled) {
        if values
            .insert(var.key.as_str(), var.value.as_str())
            .is_some()
        {
            return Err(format!("Duplicate variable: {}", var.key));
        }
    }
    let mut out = String::new();
    let mut rest = text;
    while let Some(index) = rest.find("{{") {
        out.push_str(&rest[..index]);
        let tail = &rest[index + 2..];
        let end = tail.find("}}").ok_or("Unclosed variable")?;
        let key = &tail[..end];
        out.push_str(
            values
                .get(key)
                .ok_or_else(|| format!("Undefined variable: {key}"))?,
        );
        rest = &tail[end + 2..];
        if out.len() > 2 * 1024 * 1024 {
            return Err("Resolved request exceeds 2 MiB".into());
        }
    }
    out.push_str(rest);
    if out.len() > 2 * 1024 * 1024 {
        return Err("Resolved request exceeds 2 MiB".into());
    }
    Ok(out)
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct SavedRequest {
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub timeout_secs: u64,
    pub name: String,
    pub destination: String,
    pub request: String,
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct RewriteRule {
    pub enabled: bool,
    pub response: bool,
    pub host: String,
    pub find: String,
    pub replace: String,
}
#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct ProjectState {
    pub version: u32,
    pub name: String,
    pub settings: Settings,
    pub scope: Vec<ScopeRule>,
    #[serde(default)]
    pub scope_behavior: u8,
    pub filter: HistoryFilter,
    pub presets: Vec<Preset>,
    pub variables: Vec<Variable>,
    pub replays: Vec<SavedRequest>,
    pub columns: Vec<String>,
    pub column_widths: BTreeMap<String, f32>,
    pub rules: Vec<RewriteRule>,
    pub intercept_host: String,
    pub intercept_path: String,
    pub intercept_method: String,
    pub intercept_status: String,
    pub intercept_mime: String,
}
impl ProjectState {
    pub fn migrate_scope(&mut self) {
        if self.scope_behavior == 0 {
            if !self.settings.enforce_scope {
                for rule in &mut self.scope {
                    if !rule.exclude {
                        rule.subdomains = true;
                    }
                }
            }
            self.scope_behavior = 1;
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        self.settings.validate()?;
        if self.column_widths.len() > 32
            || self
                .column_widths
                .values()
                .any(|width| !width.is_finite() || !(40.0..=2000.0).contains(width))
        {
            return Err("Column widths must be 40–2000 pixels".into());
        }
        if self.variables.len() > 256
            || self.scope.len() > 256
            || self.rules.len() > 64
            || self.presets.len() > 64
            || self.replays.len() > 8
        {
            return Err("Project configuration exceeds supported limits".into());
        }
        if serde_json::to_vec(self).map_err(|e| e.to_string())?.len() > 4 * 1024 * 1024 {
            return Err("Project configuration exceeds 4 MiB".into());
        }
        Ok(())
    }
}
impl Default for ProjectState {
    fn default() -> Self {
        Self {
            version: 1,
            name: "Untitled".into(),
            settings: Settings::default(),
            scope: vec![],
            scope_behavior: 1,
            filter: HistoryFilter::default(),
            presets: vec![
                Preset {
                    name: "All HTTP".into(),
                    filter: HistoryFilter::default(),
                },
                Preset {
                    name: "API".into(),
                    filter: HistoryFilter {
                        mime: "json".into(),
                        ..Default::default()
                    },
                },
                Preset {
                    name: "HTTP errors".into(),
                    filter: HistoryFilter {
                        errors_only: true,
                        ..Default::default()
                    },
                },
                Preset {
                    name: "Hide static assets".into(),
                    filter: HistoryFilter {
                        excluded_extensions: "css,js,map,png,jpg,jpeg,svg,gif,woff,woff2,ico"
                            .into(),
                        ..Default::default()
                    },
                },
            ],
            variables: vec![],
            replays: vec![],
            columns: [
                "#", "Method", "Status", "Protocol", "Host", "Endpoint", "Duration", "Size",
            ]
            .map(str::to_owned)
            .to_vec(),
            column_widths: BTreeMap::new(),
            rules: vec![],
            intercept_host: String::new(),
            intercept_path: String::new(),
            intercept_method: String::new(),
            intercept_status: String::new(),
            intercept_mime: String::new(),
        }
    }
}
pub fn size(bytes: u64, binary: bool) -> String {
    let base = if binary { 1024.0 } else { 1000.0 };
    let units = if binary {
        ["B", "KiB", "MiB", "GiB", "TiB", "PiB"]
    } else {
        ["B", "KB", "MB", "GB", "TB", "PB"]
    };
    let mut value = bytes as f64;
    let mut index = 0;
    while value >= base && index < units.len() - 1 {
        value /= base;
        index += 1;
    }
    if index == 0 {
        return format!("{bytes} B");
    }
    if (value * 10.0).round() / 10.0 >= base && index < units.len() - 1 {
        value /= base;
        index += 1;
    }
    format!("{value:.1} {}", units[index])
}
/// Human-readable protocol and separately inspectable HTTP/2 stream identifier.
pub fn protocol_parts(protocol: &str) -> (&str, Option<u32>) {
    if let Some(stream) = protocol.strip_prefix("HTTP/2 stream ") {
        ("HTTP/2", stream.parse().ok())
    } else {
        (protocol, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn destination_columns_preserve_path_query_and_host_boundaries() {
        assert_eq!(
            destination_parts("https://LAB.example:8443/a%2Fb.js?x=1&next=%2F#unused"),
            ("lab.example".into(), "/a%2Fb.js?x=1&next=%2F".into())
        );
        assert_eq!(
            destination_parts("https://[::1]:443/"),
            ("[::1]".into(), "/".into())
        );
        assert_eq!(
            destination_parts("example.test:443"),
            ("example.test".into(), String::new())
        );
        assert_eq!(destination_parts("connection failed"), Default::default());
        let mut state = ProjectState::default();
        state.column_widths.insert("Host".into(), 320.0);
        assert!(state.validate().is_ok());
        state.column_widths.insert("Host".into(), f32::NAN);
        assert!(state.validate().is_err());
    }
    #[test]
    fn academy_domain_scope_includes_lab_hosts_and_keeps_enforced_legacy_scopes_narrow() {
        let scope = vec![ScopeRule {
            host: "web-security-academy.net".into(),
            ..Default::default()
        }];
        assert!(in_scope(
            &scope,
            "https://0a12b3.web-security-academy.net/login"
        ));
        assert!(in_scope(&scope, "https://web-security-academy.net/"));
        assert!(!in_scope(&scope, "https://notweb-security-academy.net/"));
        let mut old = ProjectState {
            scope_behavior: 0,
            scope: vec![ScopeRule {
                host: "web-security-academy.net".into(),
                subdomains: false,
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut enforced = old.clone();
        enforced.settings.enforce_scope = true;
        enforced.migrate_scope();
        assert!(!in_scope(
            &enforced.scope,
            "https://lab.web-security-academy.net/"
        ));
        old.migrate_scope();
        assert!(in_scope(
            &old.scope,
            "https://lab.web-security-academy.net/"
        ));
    }
    #[test]
    fn scope_matches_labels_not_suffixes() {
        let rules = vec![
            ScopeRule {
                host: "example.com".into(),
                subdomains: true,
                ..Default::default()
            },
            ScopeRule {
                host: "admin.example.com".into(),
                exclude: true,
                ..Default::default()
            },
        ];
        assert!(in_scope(&rules, "https://a.example.com/x"));
        assert!(!in_scope(&rules, "https://notexample.com"));
        assert!(!in_scope(&rules, "https://admin.example.com"));
    }
    #[test]
    fn variables_do_not_expand_recursively() {
        let vars = vec![Variable {
            key: "token".into(),
            value: "{{other}}".into(),
            secret: true,
            enabled: true,
        }];
        assert_eq!(
            resolve("Bearer {{token}}", &vars).unwrap(),
            "Bearer {{other}}"
        );
        assert!(resolve("{{missing}}", &vars).is_err());
    }
    #[test]
    fn byte_units_promote_and_remain_bounded() {
        assert_eq!(size(1200, false), "1.2 KB");
        assert_eq!(size(1048576, true), "1.0 MiB");
        assert_eq!(size(999999, false), "1.0 MB");
    }
}
