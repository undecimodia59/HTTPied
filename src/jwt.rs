//! Local JWT inspection and explicit HMAC signing. Decoding never implies verification.
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use serde_json::Value;
use sha2::{Sha256, Sha384, Sha512};
pub const LIMIT: usize = 128 * 1024;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Algorithm {
    #[default]
    Hs256,
    Hs384,
    Hs512,
}
impl Algorithm {
    pub const ALL: [Self; 3] = [Self::Hs256, Self::Hs384, Self::Hs512];
    pub fn label(self) -> &'static str {
        match self {
            Self::Hs256 => "HS256",
            Self::Hs384 => "HS384",
            Self::Hs512 => "HS512",
        }
    }
}
pub struct Token {
    pub header: Value,
    pub claims: Value,
    pub signature: Vec<u8>,
    pub signing_input: String,
}
fn object(bytes: &[u8], label: &str) -> Result<Value, String> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|e| format!("Invalid {label} JSON: {e}"))?;
    if !value.is_object() {
        return Err(format!("JWT {label} must be a JSON object"));
    }
    Ok(value)
}
fn semantics(header: &Value) -> Result<(), String> {
    if header.get("crit").is_some() || header.get("b64").is_some() {
        return Err("Critical JWS extensions and unencoded payloads are not supported for signing or verification".into());
    }
    Ok(())
}
pub fn parse(text: &str) -> Result<Token, String> {
    let text = text.trim();
    if text.len() > LIMIT {
        return Err("JWT exceeds 128 KiB".into());
    }
    let parts = text.split('.').collect::<Vec<_>>();
    if parts.len() == 5 {
        return Err(
            "This is an encrypted JWE token; this workbench inspects three-part signed JWTs".into(),
        );
    }
    if parts.len() != 3 {
        return Err("JWT needs three base64url segments: header.payload.signature".into());
    }
    let decode = |value: &str| {
        URL_SAFE_NO_PAD
            .decode(value)
            .map_err(|e| format!("Invalid base64url segment: {e}"))
    };
    Ok(Token {
        header: object(&decode(parts[0])?, "header")?,
        claims: object(&decode(parts[1])?, "claims")?,
        signature: decode(parts[2])?,
        signing_input: format!("{}.{}", parts[0], parts[1]),
    })
}
fn input(header: &str, claims: &str, algorithm: &str) -> Result<String, String> {
    if header.len() + claims.len() > LIMIT {
        return Err("JWT JSON exceeds 128 KiB".into());
    }
    let mut header = object(header.as_bytes(), "header")?;
    semantics(&header)?;
    header["alg"] = Value::String(algorithm.into());
    let claims = object(claims.as_bytes(), "claims")?;
    Ok(format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header).map_err(|e| e.to_string())?),
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).map_err(|e| e.to_string())?)
    ))
}
pub fn sign(
    header: &str,
    claims: &str,
    key: &[u8],
    algorithm: Algorithm,
) -> Result<String, String> {
    if key.is_empty() || key.len() > 8192 {
        return Err("Supply a signing key between 1 byte and 8 KiB".into());
    }
    let input = input(header, claims, algorithm.label())?;
    macro_rules! tag {
        ($digest:ty) => {{
            let mut mac = Hmac::<$digest>::new_from_slice(key).map_err(|e| e.to_string())?;
            mac.update(input.as_bytes());
            mac.finalize().into_bytes().to_vec()
        }};
    }
    let tag = match algorithm {
        Algorithm::Hs256 => tag!(Sha256),
        Algorithm::Hs384 => tag!(Sha384),
        Algorithm::Hs512 => tag!(Sha512),
    };
    let output = format!("{input}.{}", URL_SAFE_NO_PAD.encode(tag));
    if output.len() > LIMIT {
        return Err("Encoded JWT exceeds 128 KiB".into());
    }
    Ok(output)
}
pub fn unsigned(header: &str, claims: &str) -> Result<String, String> {
    let output = format!("{}.", input(header, claims, "none")?);
    if output.len() > LIMIT {
        return Err("Encoded JWT exceeds 128 KiB".into());
    }
    Ok(output)
}
pub fn verify(text: &str, key: &[u8], algorithm: Algorithm) -> Result<(), String> {
    let token = parse(text)?;
    semantics(&token.header)?;
    if token.header.get("alg").and_then(Value::as_str) != Some(algorithm.label()) {
        return Err(format!(
            "Expected {}; token declares a different algorithm",
            algorithm.label()
        ));
    }
    if key.is_empty() || key.len() > 8192 {
        return Err("Supply a verification key between 1 byte and 8 KiB".into());
    }
    macro_rules! verify {
        ($digest:ty) => {{
            let mut mac = Hmac::<$digest>::new_from_slice(key).map_err(|e| e.to_string())?;
            mac.update(token.signing_input.as_bytes());
            mac.verify_slice(&token.signature)
                .map_err(|_| "Signature does not match this key".to_owned())
        }};
    }
    match algorithm {
        Algorithm::Hs256 => verify!(Sha256),
        Algorithm::Hs384 => verify!(Sha384),
        Algorithm::Hs512 => verify!(Sha512),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn known_hs256_token_verifies_and_algorithm_confusion_is_rejected() {
        let token = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyfQ.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c";
        verify(token, b"your-256-bit-secret", Algorithm::Hs256).unwrap();
        assert!(verify(token, b"wrong", Algorithm::Hs256).is_err());
        assert!(verify(token, b"your-256-bit-secret", Algorithm::Hs512).is_err());
        let parsed = parse(token).unwrap();
        let unsigned = unsigned(&parsed.header.to_string(), &parsed.claims.to_string()).unwrap();
        assert!(verify(&unsigned, b"your-256-bit-secret", Algorithm::Hs256).is_err());
    }
    #[test]
    fn edited_claims_require_a_new_signature_for_all_supported_algorithms() {
        for algorithm in Algorithm::ALL {
            let signed = sign("{}", r#"{"sub":"Київ","exp":0}"#, b"test key", algorithm).unwrap();
            verify(&signed, b"test key", algorithm).unwrap();
            let parsed = parse(&signed).unwrap();
            assert_eq!(parsed.claims["sub"], "Київ");
            let mut parts = signed.split('.').map(str::to_owned).collect::<Vec<_>>();
            parts[1] = URL_SAFE_NO_PAD.encode(br#"{"sub":"other"}"#);
            assert!(verify(&parts.join("."), b"test key", algorithm).is_err());
        }
        assert!(
            sign(
                r#"{"crit":["b64"],"b64":false}"#,
                "{}",
                b"test",
                Algorithm::Hs256
            )
            .is_err()
        );
    }
}
