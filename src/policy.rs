//! Runtime rules are published atomically; every exchange uses a consistent snapshot.
use crate::state::{ProjectState, RewriteRule, ScopeRule};
#[derive(Clone, Debug, Default)]
pub struct Policy {
    pub host: String,
    pub path: String,
    pub method: String,
    pub status: String,
    pub mime: String,
    pub responses: bool,
    pub rules: Vec<RewriteRule>,
}
impl From<&ProjectState> for Policy {
    fn from(state: &ProjectState) -> Self {
        Self {
            host: state.intercept_host.clone(),
            path: state.intercept_path.clone(),
            method: state.intercept_method.clone(),
            status: state.intercept_status.clone(),
            mime: state.intercept_mime.clone(),
            responses: state.settings.intercept_responses,
            rules: state.rules.clone(),
        }
    }
}
impl Policy {
    pub fn request(&self, target: &str, method: &str) -> bool {
        let Ok(url) = url::Url::parse(target) else {
            return false;
        };
        (self.host.is_empty()
            || ScopeRule {
                host: self.host.clone(),
                subdomains: true,
                ..Default::default()
            }
            .matches(&url))
            && (self.path.is_empty() || url.path().starts_with(&self.path))
            && (self.method.is_empty() || self.method.eq_ignore_ascii_case(method))
    }
    pub fn response(&self, target: &str, method: &str, status: u16, mime: &str) -> bool {
        self.responses
            && self.request(target, method)
            && (self.status.is_empty() || status.to_string().starts_with(&self.status))
            && (self.mime.is_empty() || mime.to_lowercase().contains(&self.mime.to_lowercase()))
    }
    pub fn rewrite(
        &self,
        target: &str,
        bytes: &[u8],
        response: bool,
    ) -> Result<(Vec<u8>, usize), String> {
        let url = url::Url::parse(target).map_err(|e| e.to_string())?;
        let mut result = bytes.to_vec();
        let mut changes = 0;
        for rule in &self.rules {
            if !rule.enabled || rule.response != response || rule.find.is_empty() {
                continue;
            }
            if !rule.host.is_empty()
                && !(ScopeRule {
                    host: rule.host.clone(),
                    subdomains: true,
                    ..Default::default()
                })
                .matches(&url)
            {
                continue;
            }
            let mut rest = result.as_slice();
            let mut out = Vec::new();
            let find = rule.find.as_bytes();
            while let Some(at) = rest.windows(find.len()).position(|w| w == find) {
                out.extend_from_slice(&rest[..at]);
                out.extend_from_slice(rule.replace.as_bytes());
                rest = &rest[at + find.len()..];
                changes += 1;
                if out.len() > 128 * 1024 {
                    return Err("Rewrite output exceeds 128 KiB".into());
                }
            }
            out.extend_from_slice(rest);
            if out.len() > 128 * 1024 {
                return Err("Rewrite output exceeds 128 KiB".into());
            }
            result = out;
        }
        Ok((result, changes))
    }
    pub fn has_response_rules(&self) -> bool {
        self.rules
            .iter()
            .any(|r| r.enabled && r.response && !r.find.is_empty())
    }
}
