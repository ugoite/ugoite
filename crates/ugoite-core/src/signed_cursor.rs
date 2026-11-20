//! Shared byte-level codec for signed client-held cursors.
//!
//! Callers retain their own typed payloads and public error variants; this
//! module only owns the common versioned base64url + HMAC envelope.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

pub(crate) enum Error {
    Invalid(&'static str),
    Tampered,
}

pub(crate) fn encode(
    version: u32,
    payload: &[u8],
    signing_key: &[u8],
    key_empty: &'static str,
    key_invalid: &'static str,
) -> Result<String, Error> {
    if signing_key.is_empty() {
        return Err(Error::Invalid(key_empty));
    }
    let mut mac =
        HmacSha256::new_from_slice(signing_key).map_err(|_| Error::Invalid(key_invalid))?;
    mac.update(payload);
    Ok(format!(
        "v{version}.{}.{}",
        URL_SAFE_NO_PAD.encode(payload),
        URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
    ))
}

pub(crate) fn decode(
    token: &str,
    signing_key: &[u8],
    kind: &'static str,
    key_empty: &'static str,
    key_invalid: &'static str,
) -> Result<Vec<u8>, Error> {
    let mut parts = token.split('.');
    let version = parts.next().ok_or(Error::Invalid(if kind == "cursor" {
        "cursor is malformed"
    } else {
        "SQL continuation is malformed"
    }))?;
    let payload = parts.next().ok_or(Error::Invalid(if kind == "cursor" {
        "cursor is malformed"
    } else {
        "SQL continuation is malformed"
    }))?;
    let signature = parts.next().ok_or(Error::Invalid(if kind == "cursor" {
        "cursor is malformed"
    } else {
        "SQL continuation is malformed"
    }))?;
    if parts.next().is_some() || version != "v1" {
        return Err(Error::Invalid(if kind == "cursor" {
            "cursor version is unsupported"
        } else {
            "SQL continuation version is unsupported"
        }));
    }
    if signing_key.is_empty() {
        return Err(Error::Invalid(key_empty));
    }
    let payload_bytes = URL_SAFE_NO_PAD.decode(payload).map_err(|_| {
        Error::Invalid(if kind == "cursor" {
            "cursor payload is malformed"
        } else {
            "SQL continuation payload is malformed"
        })
    })?;
    let signature_bytes = URL_SAFE_NO_PAD.decode(signature).map_err(|_| {
        Error::Invalid(if kind == "cursor" {
            "cursor signature is malformed"
        } else {
            "SQL continuation signature is malformed"
        })
    })?;
    let mut mac =
        HmacSha256::new_from_slice(signing_key).map_err(|_| Error::Invalid(key_invalid))?;
    mac.update(&payload_bytes);
    mac.verify_slice(&signature_bytes)
        .map_err(|_| Error::Tampered)?;
    Ok(payload_bytes)
}
