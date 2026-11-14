//! Who may connect: bearer keys, by the SHA-256 of their tokens, and the
//! Host and Origin checks that keep browsers out.

use http::HeaderMap;
use http::header::{AUTHORIZATION, HOST};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use super::Key;

/// A key and its token's hash, decoded.
#[derive(Debug, Clone)]
pub(crate) struct Credential {
    pub key: Key,
    pub digest: [u8; 32],
}

impl Credential {
    /// The key, if its hash is 64 hex digits and its budget a number of
    /// dollars; otherwise what's wrong.
    pub fn new(key: Key) -> Result<Self, String> {
        let digest = decode_hex(key.sha256.trim()).ok_or_else(|| {
            format!(
                "key {:?}: its sha256 must be 64 hex digits (the SHA-256 of its token)",
                key.id
            )
        })?;
        if !(key.daily_budget_usd.is_finite() && key.daily_budget_usd >= 0.0) {
            return Err(format!(
                "key {:?}: its daily budget must be zero or more US dollars, not {}",
                key.id, key.daily_budget_usd
            ));
        }
        Ok(Credential { key, digest })
    }
}

/// The SHA-256 of `token`, as lowercase hex: what [`Key::sha256`] holds.
pub fn sha256_hex(token: &str) -> String {
    Sha256::digest(token.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn decode_hex(s: &str) -> Option<[u8; 32]> {
    let bytes = s.as_bytes();
    if bytes.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, pair) in bytes.chunks(2).enumerate() {
        let digit = |c: u8| (c as char).to_digit(16);
        out[i] = (digit(pair[0])? * 16 + digit(pair[1])?) as u8;
    }
    Some(out)
}

/// The key whose hash is the bearer token's, if any. The token's hash is
/// compared with every key's in constant time (`subtle`), all of them
/// whatever matches, so timing says nothing about how close a guess came.
pub(crate) fn authenticate<'a, C>(credentials: &'a [C], headers: &HeaderMap) -> Option<&'a C>
where
    C: AsRef<Credential>,
{
    let token = bearer(headers)?;
    let digest: [u8; 32] = Sha256::digest(token).into();
    let mut found = None;
    for c in credentials {
        let matches: bool = c.as_ref().digest.ct_eq(&digest).into();
        if matches && found.is_none() {
            found = Some(c);
        }
    }
    found
}

/// `Authorization: Bearer <token>`'s token, if non-empty.
fn bearer(headers: &HeaderMap) -> Option<&[u8]> {
    let value = headers.get(AUTHORIZATION)?.as_bytes();
    let (scheme, token) = value.split_at_checked(7)?;
    if !scheme.eq_ignore_ascii_case(b"bearer ") {
        return None;
    }
    let token = token.trim_ascii();
    (!token.is_empty()).then_some(token)
}

/// Whether `Host` names this server on loopback: `127.0.0.1:<port>` or
/// `localhost:<port>`. A name that merely resolves here (DNS rebinding)
/// doesn't.
pub(crate) fn host_allowed(headers: &HeaderMap, port: u16) -> bool {
    let Some(host) = headers.get(HOST).and_then(|h| h.to_str().ok()) else {
        return false;
    };
    let host = host.to_ascii_lowercase();
    host == format!("127.0.0.1:{port}") || host == format!("localhost:{port}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(*k, v.parse().unwrap());
        }
        h
    }

    fn key(id: &str, token: &str) -> Credential {
        Credential::new(Key {
            id: id.into(),
            name: id.into(),
            scopes: vec![],
            sha256: sha256_hex(token),
            daily_budget_usd: 1.0,
        })
        .unwrap()
    }

    impl AsRef<Credential> for Credential {
        fn as_ref(&self) -> &Credential {
            self
        }
    }

    #[test]
    fn a_bearer_token_finds_its_key_by_hash() {
        let keys = [key("a", "token-a"), key("b", "token-b")];
        let found = |auth: &str| {
            authenticate(&keys, &headers(&[("authorization", auth)])).map(|c| c.key.id.clone())
        };
        assert_eq!(found("Bearer token-b").as_deref(), Some("b"));
        assert_eq!(found("bearer token-a").as_deref(), Some("a"));
        assert_eq!(found("Bearer token-c"), None);
        assert_eq!(found("Bearer "), None);
        assert_eq!(found("Basic token-a"), None);
        assert_eq!(authenticate(&keys, &HeaderMap::new()).map(|_| ()), None);
    }

    #[test]
    fn hashes_are_hex_and_budgets_dollars() {
        assert_eq!(
            sha256_hex("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let mut k = key("a", "t").key;
        k.sha256 = k.sha256.to_uppercase();
        assert!(Credential::new(k.clone()).is_ok());
        k.sha256.pop();
        assert!(Credential::new(k.clone()).is_err());
        k.sha256 = "zz".repeat(32);
        assert!(Credential::new(k.clone()).is_err());
        let mut k = key("a", "t").key;
        k.daily_budget_usd = f64::NAN;
        assert!(Credential::new(k).is_err());
    }

    #[test]
    fn only_loopback_hosts_with_the_port_are_allowed() {
        let allowed = |host: &str| host_allowed(&headers(&[("host", host)]), 23130);
        assert!(allowed("127.0.0.1:23130"));
        assert!(allowed("localhost:23130"));
        assert!(allowed("LOCALHOST:23130"));
        assert!(!allowed("localhost"));
        assert!(!allowed("127.0.0.1:80"));
        assert!(!allowed("evil.example:23130"));
        assert!(!allowed("[::1]:23130"));
        assert!(!host_allowed(&HeaderMap::new(), 23130));
    }
}
