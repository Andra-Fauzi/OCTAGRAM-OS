//! Minimal DNS resolver over UDP (RFC 1035).

use core::sync::atomic::{AtomicU16, Ordering};

static NEXT_TRANSACTION_ID: AtomicU16 = AtomicU16::new(0x1000);

/// Resolve a hostname (or dotted-decimal IPv4 string) to an IPv4 address.
pub fn resolve(hostname: &str) -> Result<[u8; 4], &'static str> {
    let hostname = hostname.trim();
    if hostname.is_empty() {
        return Err("empty hostname");
    }

    // Direct IPv4 literal (e.g. "192.168.43.1")
    if let Some(ip) = parse_ipv4_literal(hostname) {
        return Ok(ip);
    }

    let config = super::config();
    if !config.configured {
        return Err("network interface is not configured (run `net dhcp`)");
    }

    let dns_server = if config.dns != [0; 4] {
        config.dns
    } else if config.gateway != [0; 4] {
        config.gateway
    } else {
        [8, 8, 8, 8]
    };

    let socket = super::create_udp_socket();
    let _ = super::bind_udp_socket(&socket, 0)?;

    let txid = NEXT_TRANSACTION_ID.fetch_add(1, Ordering::Relaxed);
    let mut query_buf = [0u8; 512];
    let query_len = build_dns_query(hostname.as_bytes(), txid, &mut query_buf)?;

    super::send_udp_socket(&socket, dns_server, 53, &query_buf[..query_len])?;

    // Wait for response (up to 3 seconds: 300 iterations * 10 ms)
    let mut response_buf = [0u8; 1024];
    for _ in 0..300 {
        unsafe {
            crate::drivers::poll_deferred();
        }
        if let Ok((len, src_ip, src_port)) = super::receive_udp_socket(&socket, &mut response_buf) {
            if (src_ip == dns_server || src_ip == config.gateway) && src_port == 53 {
                if let Ok(ip) = parse_dns_response(&response_buf[..len], txid) {
                    return Ok(ip);
                }
            }
        }
        crate::arch::x86_64::delay_milliseconds(10);
    }

    Err("DNS query timed out")
}

fn parse_ipv4_literal(text: &str) -> Option<[u8; 4]> {
    let mut octets = [0u8; 4];
    let mut count = 0;
    for part in text.split('.') {
        if count >= 4 {
            return None;
        }
        let value = part.parse::<u8>().ok()?;
        octets[count] = value;
        count += 1;
    }
    if count == 4 {
        Some(octets)
    } else {
        None
    }
}

fn build_dns_query(hostname: &[u8], id: u16, output: &mut [u8; 512]) -> Result<usize, &'static str> {
    if hostname.is_empty() || hostname.len() > 254 {
        return Err("invalid hostname length");
    }
    let name = hostname.strip_suffix(b".").unwrap_or(hostname);
    if name.is_empty() || name.len() > 253 {
        return Err("invalid hostname length");
    }

    output[..12].fill(0);
    output[0..2].copy_from_slice(&id.to_be_bytes());
    output[2..4].copy_from_slice(&0x0100u16.to_be_bytes()); // Standard query with recursion desired
    output[4..6].copy_from_slice(&1u16.to_be_bytes()); // QDCOUNT = 1

    let mut cursor = 12;
    for label in name.split(|b| *b == b'.') {
        if label.is_empty() || label.len() > 63 {
            return Err("invalid DNS label length");
        }
        output[cursor] = label.len() as u8;
        cursor += 1;
        output[cursor..cursor + label.len()].copy_from_slice(label);
        cursor += label.len();
    }
    output[cursor] = 0; // Terminating zero length
    cursor += 1;

    // QTYPE = 1 (A), QCLASS = 1 (IN)
    output[cursor..cursor + 4].copy_from_slice(&[0, 1, 0, 1]);
    Ok(cursor + 4)
}

fn parse_dns_response(packet: &[u8], id: u16) -> Result<[u8; 4], &'static str> {
    if packet.len() < 12 || u16::from_be_bytes([packet[0], packet[1]]) != id {
        return Err("DNS transaction ID mismatch");
    }
    let flags = u16::from_be_bytes([packet[2], packet[3]]);
    if flags & 0x8000 == 0 {
        return Err("not a DNS response");
    }
    if flags & 0x000F == 3 {
        return Err("domain name does not exist (NXDOMAIN)");
    }
    if flags & 0x000F != 0 {
        return Err("DNS server returned error");
    }

    let qdcount = u16::from_be_bytes([packet[4], packet[5]]) as usize;
    let ancount = u16::from_be_bytes([packet[6], packet[7]]) as usize;
    if ancount == 0 {
        return Err("no DNS answers returned");
    }

    let mut cursor = 12;
    // Skip questions
    for _ in 0..qdcount {
        cursor = skip_dns_name(packet, cursor).ok_or("malformed DNS question")?;
        cursor = cursor.checked_add(4).ok_or("malformed DNS question")?;
        if cursor > packet.len() {
            return Err("truncated DNS question");
        }
    }

    // Parse answers
    for _ in 0..ancount {
        cursor = skip_dns_name(packet, cursor).ok_or("malformed DNS answer name")?;
        if cursor + 10 > packet.len() {
            return Err("truncated DNS answer header");
        }
        let rtype = u16::from_be_bytes([packet[cursor], packet[cursor + 1]]);
        let rclass = u16::from_be_bytes([packet[cursor + 2], packet[cursor + 3]]);
        let rdlength = u16::from_be_bytes([packet[cursor + 8], packet[cursor + 9]]) as usize;
        cursor += 10;
        if cursor + rdlength > packet.len() {
            return Err("truncated DNS RDATA");
        }
        if rtype == 1 && rclass == 1 && rdlength == 4 {
            let mut ip = [0u8; 4];
            ip.copy_from_slice(&packet[cursor..cursor + 4]);
            return Ok(ip);
        }
        cursor += rdlength;
    }

    Err("no IPv4 address found for host")
}

fn skip_dns_name(packet: &[u8], offset: usize) -> Option<usize> {
    let mut cursor = offset;
    let mut next = None;
    let mut pointer_count = 0usize;
    loop {
        let length = *packet.get(cursor)?;
        if length == 0 {
            return Some(next.unwrap_or(cursor + 1));
        }
        if length & 0xC0 == 0xC0 {
            let second = *packet.get(cursor + 1)?;
            let target = (((length & 0x3F) as usize) << 8) | (second as usize);
            if target >= packet.len() {
                return None;
            }
            next.get_or_insert(cursor + 2);
            cursor = target;
            pointer_count += 1;
            if pointer_count > 16 {
                return None;
            }
            continue;
        }
        if length & 0xC0 != 0 {
            return None;
        }
        cursor = cursor.checked_add(1 + length as usize)?;
        if cursor > packet.len() {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{build_dns_query, parse_dns_response, parse_ipv4_literal};

    #[test]
    fn parses_ipv4_strings() {
        assert_eq!(parse_ipv4_literal("1.2.3.4"), Some([1, 2, 3, 4]));
        assert_eq!(parse_ipv4_literal("192.168.1.1"), Some([192, 168, 1, 1]));
        assert_eq!(parse_ipv4_literal("example.com"), None);
    }

    #[test]
    fn builds_standard_dns_query() {
        let mut buf = [0u8; 512];
        let len = build_dns_query(b"google.com", 0x1234, &mut buf).unwrap();
        assert_eq!(&buf[0..2], &0x1234u16.to_be_bytes());
        assert_eq!(&buf[2..4], &0x0100u16.to_be_bytes());
        assert_eq!(buf[12], 6);
        assert_eq!(&buf[13..19], b"google");
        assert_eq!(buf[19], 3);
        assert_eq!(&buf[20..23], b"com");
        assert_eq!(buf[23], 0);
        assert_eq!(len, 28);
    }

    #[test]
    fn parses_standard_dns_response() {
        let response = [
            0x12, 0x34, // ID
            0x81, 0x80, // Flags
            0x00, 0x01, // QDCOUNT = 1
            0x00, 0x01, // ANCOUNT = 1
            0x00, 0x00, 0x00, 0x00, // NSCOUNT, ARCOUNT
            // Question: google.com IN A
            6, b'g', b'o', b'o', b'g', b'l', b'e', 3, b'c', b'o', b'm', 0,
            0x00, 0x01, 0x00, 0x01,
            // Answer: pointer 0xc00c, Type 1, Class 1, TTL 60, rdlength 4, IP 142.250.190.46
            0xc0, 0x0c, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x3c, 0x00, 0x04,
            142, 250, 190, 46,
        ];
        let ip = parse_dns_response(&response, 0x1234).unwrap();
        assert_eq!(ip, [142, 250, 190, 46]);
    }
}
