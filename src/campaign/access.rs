use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Identity {
    pub name: String,
    pub headers: String,
}
impl Default for Identity {
    fn default() -> Self {
        Self {
            name: "Low privilege".into(),
            headers: String::new(),
        }
    }
}
impl Identity {
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() || self.name.len() > 64 || self.headers.len() > 16 * 1024 {
            return Err(
                "Identity needs a name (≤64 characters) and ≤16 KiB of credential headers".into(),
            );
        }
        parse_headers(&self.headers).map(|_| ())
    }
}
pub fn parse_headers(text: &str) -> Result<Vec<(String, String)>, String> {
    text.lines().filter(|line|!line.trim().is_empty()).map(|line|{
        let (name,value)=line.split_once(':').ok_or("Credentials must be Name: value headers, one per line")?;
        let name=name.trim();
        if name.is_empty()||!name.bytes().all(|b|b.is_ascii_alphanumeric()||b"!#$%&'*+-.^_`|~".contains(&b))||matches!(name.to_ascii_lowercase().as_str(),"host"|"content-length"|"transfer-encoding"|"connection"|"proxy-connection")||value.contains(['\r','\n','\0']){return Err("Invalid credential header; destination and framing headers cannot be overridden".into());}
        Ok((name.into(),value.trim().into()))
    }).collect()
}
/// Replace every occurrence, including continuation lines. Body bytes stay unchanged.
pub fn headers(
    request: &str,
    remove: &[String],
    add: &[(String, String)],
) -> Result<String, String> {
    let (head, body) = request
        .split_once("\r\n\r\n")
        .or_else(|| request.split_once("\n\n"))
        .ok_or("Request needs a blank line before its body")?;
    let mut lines = head.lines();
    let first = lines.next().ok_or("Request line missing")?;
    let mut result = format!("{first}\r\n");
    let mut skip = false;
    for line in lines {
        if line.starts_with([' ', '\t']) {
            if !skip {
                result.push_str(line);
                result.push_str("\r\n");
            }
            continue;
        }
        let (name, _) = line.split_once(':').ok_or("Invalid request header")?;
        skip = remove.iter().any(|v| v.eq_ignore_ascii_case(name))
            || add.iter().any(|(v, _)| v.eq_ignore_ascii_case(name));
        if !skip {
            result.push_str(line);
            result.push_str("\r\n");
        }
    }
    for (name, value) in add {
        result.push_str(&format!("{name}: {value}\r\n"));
    }
    result.push_str("\r\n");
    result.push_str(body);
    Ok(result)
}
pub fn assess(
    baseline: &crate::model::WireExecution,
    variant: &crate::model::WireExecution,
    marker: &str,
) -> String {
    if !(200..300).contains(&baseline.response_status) {
        return "Inconclusive: baseline is not successful".into();
    }
    let body = super::sequence::final_message(&variant.response)
        .and_then(|message| crate::decoding::decoded_bytes(message, 256 * 1024));
    if matches!(variant.response_status, 401 | 403) {
        return "Denied by HTTP status".into();
    }
    if !marker.is_empty()
        && body
            .as_ref()
            .is_ok_and(|v| v.windows(marker.len()).any(|w| w == marker.as_bytes()))
    {
        return "Denied by configured marker".into();
    }
    if baseline.response_truncated || variant.response_truncated {
        return "Inconclusive: truncated capture".into();
    }
    if (200..300).contains(&variant.response_status)
        && baseline.response_status == variant.response_status
        && let (Ok(a), Ok(b)) = (
            super::sequence::final_message(&baseline.response)
                .and_then(|message| crate::decoding::decoded_bytes(message, 256 * 1024)),
            body,
        )
        && !a.is_empty()
        && a == b
    {
        return "Potential bypass: identical successful body".into();
    }
    "Changed response: review manually".into()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identities_remove_duplicate_and_folded_credentials_without_changing_the_body() {
        let request = "POST / HTTP/1.1\r\nAuthorization: Bearer high\r\n continuation\r\nCookie: a=1\r\nAuthorization: another\r\nX-Other: retained\r\n\r\nAuthorization: body";
        let changed = headers(
            request,
            &["Authorization".into(), "Cookie".into()],
            &[("Authorization".into(), "Bearer low".into())],
        )
        .unwrap();
        assert!(
            !changed.contains("high")
                && !changed.contains("continuation")
                && !changed.contains("Cookie:")
        );
        assert_eq!(changed.matches("Bearer low").count(), 1);
        assert!(changed.ends_with("\r\n\r\nAuthorization: body"));
        assert!(changed.contains("X-Other: retained"));
    }
}
