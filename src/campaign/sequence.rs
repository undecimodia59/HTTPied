use super::{Step, Variable};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum Source {
    #[default]
    Json,
    Header,
    Cookie,
    Regex,
}
impl Source {
    pub fn label(self) -> &'static str {
        match self {
            Self::Json => "JSON pointer",
            Self::Header => "Response header",
            Self::Cookie => "Set-Cookie",
            Self::Regex => "Body regex (capture 1)",
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Extractor {
    pub name: String,
    pub source: Source,
    pub selector: String,
    pub required: bool,
}
impl Default for Extractor {
    fn default() -> Self {
        Self {
            name: "token".into(),
            source: Source::Json,
            selector: "/token".into(),
            required: true,
        }
    }
}
impl Extractor {
    pub fn validate(&self) -> Result<(), String> {
        if self.name.is_empty()
            || self.name.len() > 64
            || !self
                .name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
            || self.selector.len() > 2048
        {
            return Err("Extraction needs a variable name (letters, numbers, underscore; ≤64) and a selector ≤2 KiB".into());
        }
        if self.source == Source::Regex {
            pattern(&self.selector)?;
        }
        Ok(())
    }
    pub fn extract(&self, response: &[u8]) -> Result<String, String> {
        let response = final_message(response)?;
        let value = match self.source {
            Source::Json => {
                let bytes = crate::decoding::decoded_bytes(response, 256 * 1024)?;
                let value: serde_json::Value = serde_json::from_slice(&bytes)
                    .map_err(|e| format!("Invalid JSON response: {e}"))?;
                let value = value
                    .pointer(&self.selector)
                    .ok_or("JSON pointer did not match")?;
                value
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| value.to_string())
            }
            Source::Header => response_headers(response)?
                .into_iter()
                .find(|(key, _)| key.eq_ignore_ascii_case(&self.selector))
                .map(|(_, value)| value)
                .ok_or("Response header not found")?,
            Source::Cookie => response_headers(response)?
                .into_iter()
                .filter(|(key, _)| key.eq_ignore_ascii_case("set-cookie"))
                .filter_map(|(_, value)| cookie_store::RawCookie::parse(value).ok())
                .find(|cookie| cookie.name() == self.selector)
                .map(|cookie| cookie.value().to_owned())
                .ok_or("Response cookie not found")?,
            Source::Regex => {
                let bytes = crate::decoding::decoded_bytes(response, 256 * 1024)?;
                let body = std::str::from_utf8(&bytes).map_err(|_| "Response body is not UTF-8")?;
                let re = pattern(&self.selector)?;
                let found = re.captures(body).ok_or("Body regex did not match")?;
                found
                    .get(1)
                    .or_else(|| found.get(0))
                    .ok_or("Regex capture missing")?
                    .as_str()
                    .to_owned()
            }
        };
        if value.len() > 8192 {
            return Err("Extracted value exceeds 8 KiB".into());
        }
        Ok(value)
    }
}
fn pattern(text: &str) -> Result<regex::Regex, String> {
    regex::RegexBuilder::new(text)
        .size_limit(1024 * 1024)
        .dfa_size_limit(1024 * 1024)
        .build()
        .map_err(|e| format!("Invalid extraction regex: {e}"))
}
pub fn final_message(mut bytes: &[u8]) -> Result<&[u8], String> {
    for _ in 0..16 {
        let mut headers = [httparse::EMPTY_HEADER; 128];
        let mut response = httparse::Response::new(&mut headers);
        let end = match response.parse(bytes).map_err(|e| e.to_string())? {
            httparse::Status::Complete(end) => end,
            _ => return Err("Incomplete response headers".into()),
        };
        if response
            .code
            .is_some_and(|c| (100..200).contains(&c) && c != 101)
        {
            bytes = &bytes[end..];
        } else {
            return Ok(bytes);
        }
    }
    Err("Too many informational responses".into())
}
pub fn response_headers(bytes: &[u8]) -> Result<Vec<(String, String)>, String> {
    let bytes = final_message(bytes)?;
    let end = bytes
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("Incomplete response")?;
    let head = std::str::from_utf8(&bytes[..end]).map_err(|_| "Response headers are not UTF-8")?;
    Ok(head
        .lines()
        .skip(1)
        .filter_map(|line| line.split_once(':'))
        .map(|(k, v)| (k.to_owned(), v.trim().to_owned()))
        .collect())
}
pub struct Context {
    pub variables: Vec<Variable>,
    cookies: BTreeMap<String, cookie_store::CookieStore>,
    managed: BTreeMap<String, Vec<cookie_store::Cookie<'static>>>,
}
impl Context {
    pub fn new(variables: Vec<Variable>) -> Self {
        Self {
            variables,
            cookies: BTreeMap::new(),
            managed: BTreeMap::new(),
        }
    }
    pub fn resolve(&self, step: &Step, carry: bool) -> Result<(Vec<u8>, url::Url), String> {
        let destination = crate::state::resolve(&step.destination, &self.variables)?;
        let mut request = crate::state::resolve(&step.request, &self.variables)?;
        let initial =
            crate::replay::build(&destination, &request, true).map_err(|e| e.to_string())?;
        let target = initial
            .split(|b| *b == b' ')
            .nth(1)
            .ok_or("Request target missing")?;
        let target = url::Url::parse(std::str::from_utf8(target).map_err(|_| "Invalid target")?)
            .map_err(|e| e.to_string())?;
        if carry && let Some(jar) = self.cookies.get(&target.origin().ascii_serialization()) {
            let fresh: Vec<_> = jar.get_request_values(&target).collect();
            let managed = self.managed.get(&target.origin().ascii_serialization());
            if !fresh.is_empty()
                || managed
                    .is_some_and(|cookies| cookies.iter().any(|cookie| cookie.matches(&target)))
            {
                let head = request
                    .split_once("\r\n\r\n")
                    .or_else(|| request.split_once("\n\n"))
                    .map(|(h, _)| h)
                    .ok_or("Incomplete request")?;
                let mut values: Vec<(String, String)> = head
                    .lines()
                    .skip(1)
                    .filter_map(|line| line.split_once(':'))
                    .filter(|(name, _)| name.eq_ignore_ascii_case("cookie"))
                    .flat_map(|(_, v)| v.split(';'))
                    .filter_map(|part| part.trim().split_once('='))
                    .map(|(k, v)| (k.into(), v.into()))
                    .collect();
                values.retain(|(name, _)| {
                    !fresh.iter().any(|(key, _)| key == name)
                        && !managed.is_some_and(|cookies| {
                            cookies
                                .iter()
                                .any(|cookie| cookie.name() == name && cookie.matches(&target))
                        })
                });
                values.extend(fresh.into_iter().map(|(k, v)| (k.into(), v.into())));
                let value = values
                    .into_iter()
                    .map(|(k, v)| format!("{k}={v}"))
                    .collect::<Vec<_>>()
                    .join("; ");
                let added = if value.is_empty() {
                    vec![]
                } else {
                    vec![("Cookie".into(), value)]
                };
                request = super::access::headers(&request, &["Cookie".into()], &added)?;
            }
        }
        Ok((
            crate::replay::build(&destination, &request, true).map_err(|e| e.to_string())?,
            target,
        ))
    }
    pub fn receive(
        &mut self,
        step: &Step,
        target: &url::Url,
        response: &[u8],
        carry: bool,
    ) -> Result<Vec<String>, String> {
        if carry {
            let jar = self
                .cookies
                .entry(target.origin().ascii_serialization())
                .or_default();
            for (_, value) in response_headers(response)?
                .into_iter()
                .filter(|(key, _)| key.eq_ignore_ascii_case("set-cookie"))
            {
                if value.len() > 4096 {
                    return Err("Response cookie exceeds 4 KiB".into());
                }
                // Keep scoped tombstones too, so an expired Set-Cookie cannot resurrect
                // a stale Cookie header from a captured request template.
                if let Ok(cookie) = cookie_store::Cookie::parse(value.as_str(), target) {
                    let cookie = cookie.into_owned();
                    let managed = self
                        .managed
                        .entry(target.origin().ascii_serialization())
                        .or_default();
                    managed.retain(|previous| {
                        previous.name() != cookie.name()
                            || previous.path != cookie.path
                            || previous.domain != cookie.domain
                    });
                    managed.push(cookie);
                    if managed.len() > 128 {
                        return Err("Sequence cookie limit exceeded (128 per origin)".into());
                    }
                }
                let _ = jar.parse(&value, target);
            }
            if jar.iter_any().count() > 128 {
                return Err("Sequence cookie limit exceeded (128 per origin)".into());
            }
        }
        let mut extracted = Vec::new();
        let mut pending = Vec::new();
        // Clear all outputs before extraction so failures cannot reuse old cycle values.
        self.variables.retain(|v| {
            !step
                .extractors
                .iter()
                .any(|extractor| extractor.name == v.key)
        });
        for extractor in &step.extractors {
            match extractor.extract(response) {
                Ok(value) => {
                    pending.push(Variable {
                        key: extractor.name.clone(),
                        value,
                        secret: true,
                        enabled: true,
                    });
                    extracted.push(extractor.name.clone());
                }
                Err(error) if extractor.required => {
                    return Err(format!("Required variable '{}': {error}", extractor.name));
                }
                Err(_) => {}
            }
            if self.variables.len() + pending.len() > 256 {
                return Err("Sequence supports at most 256 variables".into());
            }
        }
        self.variables.extend(pending);
        Ok(extracted)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deleted_cookies_do_not_reappear_from_captured_request_templates() {
        let mut context = Context::new(vec![]);
        let step = Step {
            destination: "https://local".into(),
            request: "GET / HTTP/1.1\r\nCookie: sid=stale\r\n\r\n".into(),
            ..Default::default()
        };
        let target = url::Url::parse("https://local/logout").unwrap();
        context.receive(&step,&target,b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nSet-Cookie: sid=deleted; Max-Age=0; Path=/\r\n\r\n",true).unwrap();
        let request = String::from_utf8(context.resolve(&step, true).unwrap().0).unwrap();
        assert!(
            !request.contains("Cookie:"),
            "Deleted cookie was resurrected: {request}"
        );
    }
    #[test]
    fn required_failure_discards_all_partial_outputs_and_previous_values() {
        let mut context = Context::new(vec![Variable {
            key: "token".into(),
            value: "stale".into(),
            secret: true,
            enabled: true,
        }]);
        let step = Step {
            extractors: vec![
                Extractor::default(),
                Extractor {
                    name: "other".into(),
                    selector: "/missing".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let error = context
            .receive(
                &step,
                &url::Url::parse("http://local/").unwrap(),
                b"HTTP/1.1 200 OK\r\nContent-Length: 15\r\n\r\n{\"token\":\"new\"}",
                false,
            )
            .unwrap_err();
        assert!(error.contains("other"));
        assert!(context.variables.is_empty());
    }
    #[test]
    fn extraction_decodes_chunked_json_and_preserves_typed_values() {
        let message=b"HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\ne\r\n{\"token\":\"ok\"}\r\n0\r\n\r\n";
        assert_eq!(Extractor::default().extract(message).unwrap(), "ok");
    }
    #[test]
    fn cookies_are_merged_by_path_secure_and_origin_and_optional_values_cannot_go_stale() {
        let mut context = Context::new(vec![Variable {
            key: "missing".into(),
            value: "stale".into(),
            secret: true,
            enabled: true,
        }]);
        let mut step = Step {
            destination: "https://local".into(),
            request: "GET /account/next HTTP/1.1\r\nCookie: sid=old; other=retained\r\n\r\n".into(),
            ..Default::default()
        };
        step.extractors.push(Extractor {
            name: "missing".into(),
            selector: "/missing".into(),
            required: false,
            ..Default::default()
        });
        context.receive(&step,&url::Url::parse("https://local/account/login").unwrap(),b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nSet-Cookie: sid=new; Path=/account; Secure\r\n\r\n{}",true).unwrap();
        let wire = String::from_utf8(context.resolve(&step, true).unwrap().0).unwrap();
        assert!(
            wire.contains("sid=new")
                && wire.contains("other=retained")
                && !wire.contains("sid=old")
        );
        step.destination = "https://elsewhere".into();
        let wire = String::from_utf8(context.resolve(&step, true).unwrap().0).unwrap();
        assert!(!wire.contains("sid=new"));
        assert!(!context.variables.iter().any(|v| v.key == "missing"));
    }
}
