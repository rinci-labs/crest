//! HTTPS requests via WinHTTP, for weather, calendar feeds, plugins, and Google.
//!
//! The WinRT HTTP client rejects the new Let's Encrypt chain (issuer "YR1") even though it is
//! valid, while WinHTTP uses Schannel like the curl bundled with Windows and accepts it.
//! Certificates are still fully checked, including revocation.

use std::ffi::c_void;

use windows::Win32::Foundation::{E_FAIL, E_INVALIDARG};
use windows::Win32::Networking::WinHttp::{
    INTERNET_DEFAULT_HTTPS_PORT, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_ADDREQ_FLAG_ADD,
    WINHTTP_ENABLE_SSL_REVOCATION, WINHTTP_FLAG_SECURE, WINHTTP_OPTION_ENABLE_FEATURE, WINHTTP_QUERY_FLAG_NUMBER,
    WINHTTP_QUERY_STATUS_CODE, WinHttpAddRequestHeaders, WinHttpCloseHandle, WinHttpConnect, WinHttpOpen,
    WinHttpOpenRequest, WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest,
    WinHttpSetOption, WinHttpSetTimeouts,
};
use windows::core::{Error, HSTRING, PCWSTR, Result};

/// A response this large is certainly not data Crest shows.
const MAX_BODY: usize = 1 << 20;

/// WinHTTP handle that closes itself on drop.
struct Handle(*mut c_void);

impl Handle {
    fn new(raw: *mut c_void) -> Result<Self> {
        if raw.is_null() { Err(Error::from_thread()) } else { Ok(Self(raw)) }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        let _ = unsafe { WinHttpCloseHandle(self.0) };
    }
}

/// An HTTPS request. Built by the callers that need more than a plain GET.
pub struct Request<'a> {
    pub method: &'a str,
    pub url: &'a str,
    /// Extra request headers, each one line ("Authorization: Bearer …").
    pub headers: &'a [String],
    /// Request body, sent as-is (JSON or a form).
    pub body: Option<&'a [u8]>,
}

/// Body of `url` (must be https) as text; fails if the status is not 200.
pub fn get(url: &str, agent: &str) -> Result<String> {
    send(&Request { method: "GET", url, headers: &[], body: None }, agent)
}

/// Body of an HTTPS request as text. Any 2xx status succeeds.
///
/// On a failure the message carries the status only; callers that need the server's own reason
/// (the OAuth endpoints explain a 400 in the body) use [`exchange`] instead.
pub fn send(request: &Request<'_>, agent: &str) -> Result<String> {
    let (status, body) = exchange(request, agent)?;
    if !(200..300).contains(&status) {
        return Err(Error::new(E_FAIL, format!("HTTP {status}")));
    }
    String::from_utf8(body).map_err(|_| Error::new(E_FAIL, "response is not UTF-8"))
}

/// Status and raw body, whether or not the request succeeded. Used where a 4xx answer carries
/// meaning (an expired token, a feed that is not shared any more).
pub fn exchange(request: &Request<'_>, agent: &str) -> Result<(u32, Vec<u8>)> {
    let rest = request.url.strip_prefix("https://").ok_or_else(|| Error::from(E_INVALIDARG))?;
    let (host, path) = match rest.split_once('/') {
        Some((host, path)) => (host, format!("/{path}")),
        None => (rest, "/".to_string()),
    };
    // A proxy or token is not a host name.
    let host = host.split('@').next_back().unwrap_or(host);
    unsafe {
        let proxy = WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY;
        let session = Handle::new(WinHttpOpen(&HSTRING::from(agent), proxy, PCWSTR::null(), PCWSTR::null(), 0))?;
        let _ = WinHttpSetTimeouts(session.0, 10_000, 10_000, 10_000, 15_000);
        let connect = Handle::new(WinHttpConnect(session.0, &HSTRING::from(host), INTERNET_DEFAULT_HTTPS_PORT, 0))?;
        let path = HSTRING::from(path);
        let verb = HSTRING::from(request.method);
        let handle = Handle::new(WinHttpOpenRequest(
            connect.0,
            &verb,
            &path,
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null(),
            WINHTTP_FLAG_SECURE,
        ))?;
        let revocation = WINHTTP_ENABLE_SSL_REVOCATION.to_ne_bytes();
        WinHttpSetOption(Some(handle.0), WINHTTP_OPTION_ENABLE_FEATURE, Some(&revocation))?;
        for header in request.headers {
            let line: Vec<u16> = header.encode_utf16().collect();
            WinHttpAddRequestHeaders(handle.0, &line, WINHTTP_ADDREQ_FLAG_ADD)?;
        }
        let length = request.body.map_or(0, |body| body.len() as u32);
        WinHttpSendRequest(handle.0, None, request.body.map(|b| b.as_ptr().cast()), length, length, 0)?;
        WinHttpReceiveResponse(handle.0, std::ptr::null_mut())?;

        let (mut status, mut size) = (0u32, size_of::<u32>() as u32);
        let query = WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER;
        let buffer = Some((&mut status as *mut u32).cast());
        WinHttpQueryHeaders(handle.0, query, PCWSTR::null(), buffer, &mut size, std::ptr::null_mut())?;

        let mut body = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            let mut read = 0u32;
            WinHttpReadData(handle.0, chunk.as_mut_ptr().cast(), chunk.len() as u32, &mut read)?;
            if read == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..read as usize]);
            if body.len() > MAX_BODY {
                return Err(Error::new(E_FAIL, "response too large"));
            }
        }
        Ok((status, body))
    }
}

/// Percent-encoding for query values and form bodies (RFC 3986, space as `%20`).
pub fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// `application/x-www-form-urlencoded` body from name/value pairs.
pub fn form(pairs: &[(&str, &str)]) -> String {
    pairs.iter().map(|(k, v)| format!("{}={}", encode(k), encode(v))).collect::<Vec<_>>().join("&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_query_values() {
        assert_eq!(encode("a b&c=d"), "a%20b%26c%3Dd");
        assert_eq!(encode("Crest/0.1 (+https://x)"), "Crest%2F0.1%20%28%2Bhttps%3A%2F%2Fx%29");
        assert_eq!(form(&[("a", "1 2"), ("b", "3")]), "a=1%202&b=3");
    }
}
