//! Minimal HTTP/1.1 client and synchronous TCP exchange over IPv4.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpUrl<'a> {
    pub host: &'a str,
    pub port: u16,
    pub path: &'a str,
}

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status_code: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn body_str(&self) -> &str {
        core::str::from_utf8(&self.body).unwrap_or("<binary data>")
    }
}

pub fn parse_url(url: &str) -> Result<HttpUrl<'_>, &'static str> {
    let url = url.trim();
    if url.is_empty() {
        return Err("empty URL");
    }
    if url.starts_with("https://") {
        return Err("HTTPS is not supported yet (use plain http://)");
    }
    let without_scheme = if let Some(stripped) = url.strip_prefix("http://") {
        stripped
    } else {
        url
    };

    let (host_port, path) = match without_scheme.find('/') {
        Some(pos) => (&without_scheme[..pos], &without_scheme[pos..]),
        None => (without_scheme, "/"),
    };

    if host_port.is_empty() {
        return Err("empty host in URL");
    }

    let (host, port) = match host_port.rfind(':') {
        Some(pos) => {
            let host = &host_port[..pos];
            let port_str = &host_port[pos + 1..];
            let port = port_str.parse::<u16>().map_err(|_| "invalid port in URL")?;
            (host, port)
        }
        None => (host_port, 80),
    };

    if host.is_empty() {
        return Err("empty host in URL");
    }

    Ok(HttpUrl { host, port, path })
}

pub fn parse_http_response(raw: &[u8]) -> Result<HttpResponse, &'static str> {
    if raw.is_empty() {
        return Err("empty HTTP response");
    }

    // Split headers and body at \r\n\r\n or \n\n
    let (header_bytes, body_bytes) = if let Some(pos) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
        (&raw[..pos], &raw[pos + 4..])
    } else if let Some(pos) = raw.windows(2).position(|w| w == b"\n\n") {
        (&raw[..pos], &raw[pos + 2..])
    } else {
        (raw, &raw[raw.len()..])
    };

    let header_str = core::str::from_utf8(header_bytes).map_err(|_| "invalid UTF-8 in HTTP headers")?;
    let mut lines = header_str.lines();

    // First line: HTTP/1.1 200 OK
    let status_line = lines.next().ok_or("missing HTTP status line")?;
    let mut parts = status_line.split_whitespace();
    let _proto = parts.next().ok_or("missing HTTP protocol in status line")?;
    let code_str = parts.next().ok_or("missing HTTP status code")?;
    let status_code = code_str.parse::<u16>().map_err(|_| "invalid HTTP status code")?;

    let mut headers = Vec::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        if let Some((key, val)) = line.split_once(':') {
            headers.push((String::from(key.trim()), String::from(val.trim())));
        }
    }

    Ok(HttpResponse {
        status_code,
        headers,
        body: body_bytes.to_vec(),
    })
}

/// Perform a synchronous raw TCP exchange: connect, send payload, receive all data until EOF or timeout, close.
pub fn tcp_exchange(remote_ip: [u8; 4], remote_port: u16, data: &[u8], timeout_ms: u64) -> Result<Vec<u8>, &'static str> {
    let socket = super::create_tcp_socket();
    let res = super::connect_tcp_socket(&socket, remote_ip, remote_port);
    if let Err(e) = res {
        if e != "TCP connection is in progress" {
            return Err(e);
        }
    }

    // Wait for connection to establish
    let connect_loops = (timeout_ms / 10).max(100);
    let mut connected = false;
    for _ in 0..connect_loops {
        unsafe {
            crate::drivers::poll_deferred();
        }
        super::poll_tcp();
        if super::tcp_is_established(&socket) {
            connected = true;
            break;
        }
        if super::tcp_is_closed(&socket) {
            let err = super::tcp_socket_error(&socket).unwrap_or("TCP connection reset");
            return Err(err);
        }
        crate::arch::x86_64::delay_milliseconds(10);
    }

    if !connected {
        super::close_tcp_socket(&socket);
        return Err("TCP connection timed out");
    }

    // Send payload
    let mut remaining = data;
    let mut send_timeout = 500usize;
    while !remaining.is_empty() && send_timeout > 0 {
        match super::send_tcp_socket(&socket, remaining) {
            Ok(n) => {
                remaining = &remaining[n..];
            }
            Err("TCP send would block") => {
                unsafe { crate::drivers::poll_deferred(); }
                super::poll_tcp();
                crate::arch::x86_64::delay_milliseconds(10);
                send_timeout -= 1;
            }
            Err(e) => {
                super::close_tcp_socket(&socket);
                return Err(e);
            }
        }
    }

    if !remaining.is_empty() {
        super::close_tcp_socket(&socket);
        return Err("TCP send timed out");
    }

    // Receive response
    let mut output = Vec::new();
    let mut buf = [0u8; 1024];
    let mut idle_timeout = 300usize; // 3 seconds idle
    loop {
        unsafe { crate::drivers::poll_deferred(); }
        super::poll_tcp();

        match super::receive_tcp_socket(&socket, &mut buf, false) {
            Ok(0) => {
                if super::tcp_is_receive_eof(&socket) {
                    break;
                }
                crate::arch::x86_64::delay_milliseconds(10);
                idle_timeout = idle_timeout.saturating_sub(1);
                if idle_timeout == 0 {
                    break;
                }
            }
            Ok(n) => {
                output.extend_from_slice(&buf[..n]);
                idle_timeout = 300;
            }
            Err("TCP receive would block") => {
                if super::tcp_is_receive_eof(&socket) {
                    break;
                }
                crate::arch::x86_64::delay_milliseconds(10);
                idle_timeout = idle_timeout.saturating_sub(1);
                if idle_timeout == 0 {
                    break;
                }
            }
            Err(_) => break,
        }
    }

    super::close_tcp_socket(&socket);
    Ok(output)
}

/// Perform an HTTP GET request to the specified URL.
pub fn http_get(url: &str) -> Result<HttpResponse, &'static str> {
    let parsed = parse_url(url)?;
    let ip = super::dns_resolve(parsed.host)?;

    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: OCTAGRAM-OS\r\nConnection: close\r\nAccept: */*\r\n\r\n",
        parsed.path, parsed.host
    );

    let raw_response = tcp_exchange(ip, parsed.port, request.as_bytes(), 5000)?;
    parse_http_response(&raw_response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_various_urls() {
        let u1 = parse_url("http://example.com").unwrap();
        assert_eq!(u1.host, "example.com");
        assert_eq!(u1.port, 80);
        assert_eq!(u1.path, "/");

        let u2 = parse_url("example.com/test/index.html").unwrap();
        assert_eq!(u2.host, "example.com");
        assert_eq!(u2.port, 80);
        assert_eq!(u2.path, "/test/index.html");

        let u3 = parse_url("http://192.168.43.1:8080/api").unwrap();
        assert_eq!(u3.host, "192.168.43.1");
        assert_eq!(u3.port, 8080);
        assert_eq!(u3.path, "/api");

        assert!(parse_url("https://secure.com").is_err());
        assert!(parse_url("").is_err());
    }

    #[test]
    fn parses_valid_http_response() {
        let data = b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 12\r\n\r\nHello World!";
        let resp = parse_http_response(data).unwrap();
        assert_eq!(resp.status_code, 200);
        assert_eq!(resp.headers.len(), 2);
        assert_eq!(resp.headers[0], (String::from("Content-Type"), String::from("text/plain")));
        assert_eq!(resp.body, b"Hello World!");
        assert_eq!(resp.body_str(), "Hello World!");
    }
}
