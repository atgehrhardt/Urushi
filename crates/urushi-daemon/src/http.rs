//! HTTPS requests through the system's `curl` (ADR 0021, after ADR 0011).
//!
//! The daemon links no HTTP or TLS library: `curl` ships with macOS, Windows 10+ and
//! nearly every Linux, and the system's certificate store comes with it. Everything
//! about a request — address, method, headers (credentials included) and body — goes to
//! curl as a config file on **stdin** (`--config -`), so none of it appears in a process
//! listing. Requests are blocking: call them off the async runtime.
//!
//! [`HttpClient`] is the seam: services take it as a trait object, and tests answer
//! without a network.

use std::fmt::{self, Write as _};
use std::time::Duration;

use crate::exec;

/// How long a request may take, connecting included.
const TIMEOUT: Duration = Duration::from_secs(30);

/// An HTTP method.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    /// GET.
    Get,
    /// POST.
    Post,
    /// PUT.
    Put,
}

impl Method {
    fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
        }
    }
}

/// A request. `Debug` leaves out header values, which may be credentials.
#[derive(Clone, PartialEq, Eq)]
pub struct Request {
    /// The method.
    pub method: Method,
    /// The full address, query included.
    pub url: String,
    /// Headers as `(name, value)`.
    pub headers: Vec<(String, String)>,
    /// A JSON body, sent with `Content-Type: application/json`.
    pub json: Option<String>,
}

impl fmt::Debug for Request {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Request")
            .field("method", &self.method)
            .field("url", &self.url)
            .field(
                "headers",
                &self.headers.iter().map(|(n, _)| n).collect::<Vec<_>>(),
            )
            .field("json", &self.json.as_ref().map(String::len))
            .finish()
    }
}

/// An answer: any status, including errors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Response {
    /// The HTTP status.
    pub status: u16,
    /// The `Retry-After` header in seconds, if sent as a number.
    pub retry_after: Option<Duration>,
    /// The body.
    pub body: Vec<u8>,
}

/// A request that got no HTTP answer: no network, DNS, TLS, a timeout, or no `curl`.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct HttpError(pub String);

/// Sends HTTP requests.
pub trait HttpClient: Send + Sync {
    /// Sends `request` and returns the answer, whatever its status. Blocking.
    fn send(&self, request: &Request) -> Result<Response, HttpError>;
}

/// [`HttpClient`] over the system's `curl`.
#[derive(Clone, Debug)]
pub struct CurlHttp {
    program: String,
}

impl Default for CurlHttp {
    fn default() -> Self {
        Self {
            program: "curl".into(),
        }
    }
}

impl CurlHttp {
    /// A client running `program` instead of `curl` (for tests).
    pub fn with_program(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
        }
    }
}

impl HttpClient for CurlHttp {
    fn send(&self, request: &Request) -> Result<Response, HttpError> {
        let config = curl_config(request);
        let out = exec::run(
            &self.program,
            &["--config", "-"],
            Some(config.as_bytes()),
            TIMEOUT + Duration::from_secs(5),
        )
        .map_err(|e| HttpError(friendly(&e)))?;
        parse_output(&out)
    }
}

/// curl exits with an error for transport failures (it isn't given `--fail`, so HTTP
/// errors still exit 0).
fn friendly(error: &str) -> String {
    if error.starts_with("cannot run") {
        "curl isn't installed; it's needed to reach Jira".into()
    } else if error.contains("exit status: 6") {
        "the site's address couldn't be found".into()
    } else if error.contains("exit status: 7") {
        "the site refused the connection".into()
    } else if error.contains("exit status: 35") || error.contains("exit status: 60") {
        "the secure connection to the site failed".into()
    } else if error.contains("exit status: 28") || error.contains("timed out") {
        "the request timed out".into()
    } else {
        format!("couldn't connect ({error})")
    }
}

/// The curl config for a request: one `option = "value"` per line.
pub(crate) fn curl_config(request: &Request) -> String {
    let mut lines = vec![
        "silent".to_owned(),
        "show-error".to_owned(),
        "globoff".to_owned(),
        format!("max-time = {}", TIMEOUT.as_secs()),
        format!("request = {}", quote(request.method.as_str())),
        format!("url = {}", quote(&request.url)),
        format!(
            "write-out = {}",
            quote("\n%header{retry-after}\n%{http_code}")
        ),
    ];
    for (name, value) in &request.headers {
        lines.push(format!("header = {}", quote(&format!("{name}: {value}"))));
    }
    if let Some(json) = &request.json {
        lines.push(format!(
            "header = {}",
            quote("Content-Type: application/json")
        ));
        lines.push(format!("data-binary = {}", quote(json)));
    }
    let mut config = lines.join("\n");
    config.push('\n');
    config
}

/// A curl config string: quoted, with backslashes, quotes and line breaks escaped. curl
/// turns `\n`, `\r` and `\t` back into the characters, and takes `\\` and `\"` as they
/// are.
pub(crate) fn quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The answer from curl's output: the body, then the `Retry-After` header and the status
/// on a line each (the `write-out` in [`curl_config`]).
pub(crate) fn parse_output(out: &[u8]) -> Result<Response, HttpError> {
    let mut lines = out.rsplitn(3, |b| *b == b'\n');
    let status = lines.next().unwrap_or_default();
    let retry_after = lines.next().unwrap_or_default();
    let body = lines.next().unwrap_or_default();
    let status: u16 = String::from_utf8_lossy(status)
        .trim()
        .parse()
        .map_err(|_| HttpError("no answer".into()))?;
    if status == 0 {
        return Err(HttpError("no answer".into()));
    }
    Ok(Response {
        status,
        retry_after: String::from_utf8_lossy(retry_after)
            .trim()
            .parse()
            .ok()
            .map(Duration::from_secs),
        body: body.to_vec(),
    })
}

/// Percent-encodes a query parameter value (RFC 3986 unreserved characters stay).
pub fn encode_query(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

/// Standard base64 with padding, for `Authorization: Basic`.
pub fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(TABLE[(n >> (18 - 6 * i) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests;
