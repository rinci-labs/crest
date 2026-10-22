//! Small crypto helpers with Windows' own providers: random values for OAuth, SHA-256 for PKCE,
//! and DPAPI to keep a refresh token readable only by this Windows user.
//!
//! Nothing here is used for message content or files; only the Google sign-in needs it.

use windows::Win32::Security::Cryptography::{
    BCRYPT_ALG_HANDLE, BCRYPT_SHA256_ALGORITHM, BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptCloseAlgorithmProvider,
    BCryptGenRandom, BCryptHash, BCryptOpenAlgorithmProvider, CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN,
    CryptProtectData, CryptUnprotectData,
};
use windows::Win32::Foundation::{HLOCAL, LocalFree};
use windows::core::{PCWSTR, w};

/// Random bytes from the system generator.
pub fn random(bytes: &mut [u8]) -> bool {
    unsafe { BCryptGenRandom(None, bytes, BCRYPT_USE_SYSTEM_PREFERRED_RNG).is_ok() }
}

/// `len` random bytes, base64url without padding: a PKCE verifier or state value.
pub fn random_b64(len: usize) -> String {
    let mut bytes = vec![0u8; len];
    if !random(&mut bytes) {
        // The system generator is always there; falling back to a counter would be a security
        // hole, so a repeated value is better than a predictable one.
        error_log!("system random generator unavailable");
    }
    base64url(&bytes)
}

/// SHA-256 of `data`, base64url without padding: the PKCE challenge for a verifier.
pub fn sha256_b64(data: &[u8]) -> String {
    let mut digest = [0u8; 32];
    if !sha256(data, &mut digest) {
        error_log!("SHA-256 unavailable");
    }
    base64url(&digest)
}

fn sha256(data: &[u8], out: &mut [u8; 32]) -> bool {
    unsafe {
        let mut algorithm = BCRYPT_ALG_HANDLE::default();
        if BCryptOpenAlgorithmProvider(&mut algorithm, BCRYPT_SHA256_ALGORITHM, PCWSTR::null(), Default::default()).is_err() {
            return false;
        }
        // Opens the hash, feeds the data, and closes it again in one call.
        let status = BCryptHash(algorithm, None, data, out);
        let _ = BCryptCloseAlgorithmProvider(algorithm, 0);
        status.is_ok()
    }
}

/// Base64url (RFC 4648 §5) without padding, as PKCE and OAuth require.
fn base64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[(n >> 6) as usize & 63] as char);
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[n as usize & 63] as char);
        }
    }
    out
}

/// Encrypt `text` for the current Windows user (DPAPI), as base64 for the settings file.
pub fn protect(text: &str) -> Option<String> {
    let input = CRYPT_INTEGER_BLOB { cbData: text.len() as u32, pbData: text.as_ptr() as *mut u8 };
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptProtectData(&input, w!("Crest"), None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut output).ok()?;
        let bytes = std::slice::from_raw_parts(output.pbData, output.cbData as usize);
        let text = base64(bytes);
        // The copy is on the heap until LocalFree: overwrite it before it goes back to the system.
        wipe(output.pbData, output.cbData as usize);
        let _ = LocalFree(Some(HLOCAL(output.pbData.cast())));
        Some(text)
    }
}

/// Decrypt a value written by [`protect`].
pub fn unprotect(text: &str) -> Option<String> {
    let mut sealed = from_base64(text.trim())?;
    let input = CRYPT_INTEGER_BLOB { cbData: sealed.len() as u32, pbData: sealed.as_mut_ptr() };
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptUnprotectData(&input, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut output).ok()?;
        // This buffer holds the plaintext token, so it is overwritten before it is freed. The
        // returned `String` owns the only remaining copy, which the caller needs.
        let plain = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        wipe(output.pbData, output.cbData as usize);
        let _ = LocalFree(Some(HLOCAL(output.pbData.cast())));
        match String::from_utf8(plain) {
            Ok(text) => Some(text),
            Err(e) => {
                // Not text after all: the copy is about to be dropped, so scrub it first.
                let mut bytes = e.into_bytes();
                wipe(bytes.as_mut_ptr(), bytes.len());
                None
            }
        }
    }
}

/// Overwrite `len` bytes at `data` before the buffer is freed, so a secret does not linger in
/// freed memory for the next allocation to pick up.
unsafe fn wipe(data: *mut u8, len: usize) {
    if !data.is_null() && len > 0 {
        unsafe { std::ptr::write_bytes(data, 0, len) };
    }
}

/// Standard base64 with padding, for the encrypted token blob.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { ALPHABET[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { ALPHABET[n as usize & 63] as char } else { '=' });
    }
    out
}

fn from_base64(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let mut buffer = 0u32;
    let mut bits = 0u32;
    for c in text.bytes() {
        let value = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            b' ' | b'\r' | b'\n' | b'\t' => continue,
            _ => return None,
        };
        buffer = (buffer << 6) | value as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trips() {
        assert_eq!(base64(b"a"), "YQ==");
        assert_eq!(base64(b"ab"), "YWI=");
        assert_eq!(base64(b"abc"), "YWJj");
        assert_eq!(from_base64("YQ==").as_deref(), Some(&b"a"[..]));
        assert_eq!(from_base64("YWJj").as_deref(), Some(&b"abc"[..]));
    }

    #[test]
    fn base64url_has_no_padding_or_plus_slash() {
        let encoded = base64url(&[0xfb, 0xff, 0xbf]);
        assert!(!encoded.contains('='));
        assert!(!encoded.contains('+'));
        assert!(!encoded.contains('/'));
    }

    #[test]
    fn sha256_matches_known_answer() {
        // SHA-256 of the empty string, base64url.
        assert_eq!(sha256_b64(b""), "47DEQpj8HBSa-_TImW-5JCeuQeRkm5NMpJWZG3hSuFU");
        // PKCE's published example: verifier → challenge.
        assert_eq!(
            sha256_b64(b"dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn dpapi_round_trips_for_this_user() {
        let token = "1//0gExampleRefreshTokenWith-Characters";
        let sealed = protect(token).expect("DPAPI available");
        assert_ne!(sealed, token);
        assert_eq!(unprotect(&sealed).as_deref(), Some(token));
        assert!(unprotect("not encrypted").is_none());
    }

    #[test]
    fn random_values_differ() {
        assert_ne!(random_b64(32), random_b64(32));
        // 32 random bytes are 43 base64url characters: PKCE (RFC 7636 §4.2) allows 43..=128.
        let verifier = random_b64(32);
        assert_eq!(verifier.len(), 43);
        assert!((43..=128).contains(&verifier.len()), "PKCE verifier out of range: {}", verifier.len());
        assert!(verifier.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.'));
    }
}
