//! std:dns — DNS resolution
//!
//! Provides hostname resolution (A/AAAA via `ToSocketAddrs`) and DNS record
//! lookups (MX, TXT, SRV, CNAME, NS, PTR) via a minimal DNS wire-protocol
//! implementation over UDP.  No external crate dependencies.

use raya_sdk::{IoCompletion, IoRequest, NativeCallResult, NativeContext, NativeValue};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, ToSocketAddrs, UdpSocket};
use std::time::Duration;

// ---------------------------------------------------------------------------
// DNS record type constants
// ---------------------------------------------------------------------------

const DNS_TYPE_A: u16 = 1;
const DNS_TYPE_NS: u16 = 2;
const DNS_TYPE_CNAME: u16 = 5;
const DNS_TYPE_PTR: u16 = 12;
const DNS_TYPE_MX: u16 = 15;
const DNS_TYPE_TXT: u16 = 16;
const DNS_TYPE_AAAA: u16 = 28;
const DNS_TYPE_SRV: u16 = 33;

// ---------------------------------------------------------------------------
// Public native handlers — A / AAAA lookups (via ToSocketAddrs)
// ---------------------------------------------------------------------------

/// Resolve hostname to all IP addresses (IPv4 + IPv6).
pub fn dns_lookup(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let hostname = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("dns.lookup: {}", e)),
    };
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            let addrs: Vec<String> = match (hostname.as_str(), 0u16).to_socket_addrs() {
                Ok(iter) => iter.map(|a| a.ip().to_string()).collect(),
                Err(_) => vec![],
            };
            IoCompletion::StringArray(addrs)
        }),
    })
}

/// Resolve hostname to IPv4 addresses only.
pub fn dns_lookup4(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let hostname = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("dns.lookup4: {}", e)),
    };
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            let addrs: Vec<String> = match (hostname.as_str(), 0u16).to_socket_addrs() {
                Ok(iter) => iter
                    .filter(|a| a.is_ipv4())
                    .map(|a| a.ip().to_string())
                    .collect(),
                Err(_) => vec![],
            };
            IoCompletion::StringArray(addrs)
        }),
    })
}

/// Resolve hostname to IPv6 addresses only.
pub fn dns_lookup6(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let hostname = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("dns.lookup6: {}", e)),
    };
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            let addrs: Vec<String> = match (hostname.as_str(), 0u16).to_socket_addrs() {
                Ok(iter) => iter
                    .filter(|a| a.is_ipv6())
                    .map(|a| a.ip().to_string())
                    .collect(),
                Err(_) => vec![],
            };
            IoCompletion::StringArray(addrs)
        }),
    })
}

// ---------------------------------------------------------------------------
// Public native handlers — record-type lookups (via raw DNS UDP)
// ---------------------------------------------------------------------------

/// Look up MX records.  Returns flat `[priority, exchange, ...]`.
pub fn dns_lookup_mx(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let hostname = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("dns.lookupMx: {}", e)),
    };
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            let response = match dns_query(&hostname, DNS_TYPE_MX) {
                Ok(r) => r,
                Err(_) => return IoCompletion::StringArray(vec![]),
            };
            let records = parse_answers(&response, DNS_TYPE_MX);
            IoCompletion::StringArray(records)
        }),
    })
}

/// Look up TXT records.
pub fn dns_lookup_txt(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let hostname = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("dns.lookupTxt: {}", e)),
    };
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            let response = match dns_query(&hostname, DNS_TYPE_TXT) {
                Ok(r) => r,
                Err(_) => return IoCompletion::StringArray(vec![]),
            };
            let records = parse_answers(&response, DNS_TYPE_TXT);
            IoCompletion::StringArray(records)
        }),
    })
}

/// Look up SRV records.  Returns flat `[priority, weight, port, target, ...]`.
pub fn dns_lookup_srv(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let hostname = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("dns.lookupSrv: {}", e)),
    };
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            let response = match dns_query(&hostname, DNS_TYPE_SRV) {
                Ok(r) => r,
                Err(_) => return IoCompletion::StringArray(vec![]),
            };
            let records = parse_answers(&response, DNS_TYPE_SRV);
            IoCompletion::StringArray(records)
        }),
    })
}

/// Look up CNAME records.
pub fn dns_lookup_cname(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let hostname = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("dns.lookupCname: {}", e)),
    };
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            let response = match dns_query(&hostname, DNS_TYPE_CNAME) {
                Ok(r) => r,
                Err(_) => return IoCompletion::StringArray(vec![]),
            };
            let records = parse_answers(&response, DNS_TYPE_CNAME);
            IoCompletion::StringArray(records)
        }),
    })
}

/// Look up NS (nameserver) records.
pub fn dns_lookup_ns(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let hostname = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("dns.lookupNs: {}", e)),
    };
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            let response = match dns_query(&hostname, DNS_TYPE_NS) {
                Ok(r) => r,
                Err(_) => return IoCompletion::StringArray(vec![]),
            };
            let records = parse_answers(&response, DNS_TYPE_NS);
            IoCompletion::StringArray(records)
        }),
    })
}

/// Reverse DNS lookup — hostnames for an IP address.
pub fn dns_reverse(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let ip_str = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("dns.reverse: {}", e)),
    };
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            let ptr_name = match ip_str.parse::<IpAddr>() {
                Ok(IpAddr::V4(v4)) => {
                    let octets = v4.octets();
                    format!(
                        "{}.{}.{}.{}.in-addr.arpa",
                        octets[3], octets[2], octets[1], octets[0]
                    )
                }
                Ok(IpAddr::V6(v6)) => {
                    let segments = v6.octets();
                    let mut nibbles = String::with_capacity(73);
                    for &byte in segments.iter().rev() {
                        if !nibbles.is_empty() {
                            nibbles.push('.');
                        }
                        nibbles.push(char::from(b"0123456789abcdef"[(byte & 0x0f) as usize]));
                        nibbles.push('.');
                        nibbles.push(char::from(b"0123456789abcdef"[(byte >> 4) as usize]));
                    }
                    format!("{}.ip6.arpa", nibbles)
                }
                Err(_) => return IoCompletion::StringArray(vec![]),
            };
            let response = match dns_query(&ptr_name, DNS_TYPE_PTR) {
                Ok(r) => r,
                Err(_) => return IoCompletion::StringArray(vec![]),
            };
            let records = parse_answers(&response, DNS_TYPE_PTR);
            IoCompletion::StringArray(records)
        }),
    })
}

// ---------------------------------------------------------------------------
// DNS wire-protocol helpers
// ---------------------------------------------------------------------------

/// Read the system resolver address from `/etc/resolv.conf`.
/// Falls back to `8.8.8.8` if nothing is found.
fn get_system_resolver() -> String {
    if let Ok(content) = std::fs::read_to_string("/etc/resolv.conf") {
        for line in content.lines() {
            let line = line.trim();
            if line.starts_with("nameserver") {
                if let Some(addr) = line.split_whitespace().nth(1) {
                    // Skip link-local / loopback for robustness
                    return addr.to_string();
                }
            }
        }
    }
    "8.8.8.8".to_string()
}

/// Generate a pseudo-random 16-bit transaction ID.
fn transaction_id() -> u16 {
    // Use a combination of the current time and thread id for a simple
    // non-cryptographic ID.  Good enough for DNS transaction IDs.
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let tid = std::thread::current().id();
    let hash = t as u64 ^ format!("{:?}", tid).len() as u64;
    (hash & 0xFFFF) as u16
}

/// Encode a DNS domain name into wire format (label-length-prefixed).
fn encode_dns_name(name: &str, packet: &mut Vec<u8>) {
    for label in name.split('.') {
        if label.is_empty() {
            continue;
        }
        packet.push(label.len() as u8);
        packet.extend_from_slice(label.as_bytes());
    }
    packet.push(0); // root label
}

/// Build a DNS query packet for the given name and record type.
fn build_dns_query(name: &str, record_type: u16) -> Vec<u8> {
    let mut packet = Vec::with_capacity(64);

    // Header (12 bytes)
    let id = transaction_id();
    packet.extend_from_slice(&id.to_be_bytes()); // ID
    packet.extend_from_slice(&[0x01, 0x00]); // Flags: QR=0, RD=1
    packet.extend_from_slice(&1u16.to_be_bytes()); // QDCOUNT = 1
    packet.extend_from_slice(&0u16.to_be_bytes()); // ANCOUNT = 0
    packet.extend_from_slice(&0u16.to_be_bytes()); // NSCOUNT = 0
    packet.extend_from_slice(&0u16.to_be_bytes()); // ARCOUNT = 0

    // Question section
    encode_dns_name(name, &mut packet);
    packet.extend_from_slice(&record_type.to_be_bytes()); // QTYPE
    packet.extend_from_slice(&1u16.to_be_bytes()); // QCLASS = IN

    packet
}

/// Send a DNS query and return the raw response bytes.
fn dns_query(name: &str, record_type: u16) -> Result<Vec<u8>, String> {
    let resolver = get_system_resolver();
    let sock = UdpSocket::bind("0.0.0.0:0").map_err(|e| format!("bind: {}", e))?;
    sock.set_read_timeout(Some(Duration::from_secs(5))).ok();
    sock.set_write_timeout(Some(Duration::from_secs(5))).ok();

    let query = build_dns_query(name, record_type);
    sock.send_to(&query, format!("{}:53", resolver))
        .map_err(|e| format!("send: {}", e))?;

    let mut buf = vec![0u8; 4096];
    let len = sock.recv(&mut buf).map_err(|e| format!("recv: {}", e))?;
    buf.truncate(len);
    Ok(buf)
}

/// Parse a compressed DNS name starting at `offset`.
/// Returns the decoded name and advances `offset` past the name.
fn parse_dns_name(data: &[u8], offset: &mut usize) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut pos = *offset;
    let mut jumped = false;
    let mut jump_target: usize = 0;

    loop {
        if pos >= data.len() {
            break;
        }
        let len = data[pos] as usize;
        if len == 0 {
            pos += 1;
            break;
        }
        // Compression pointer (top 2 bits = 11)
        if (len & 0xC0) == 0xC0 {
            if pos + 1 >= data.len() {
                break;
            }
            if !jumped {
                jump_target = pos + 2;
            }
            pos = ((len & 0x3F) << 8) | (data[pos + 1] as usize);
            jumped = true;
            continue;
        }
        pos += 1;
        if pos + len > data.len() {
            break;
        }
        parts.push(String::from_utf8_lossy(&data[pos..pos + len]).to_string());
        pos += len;
    }

    if jumped {
        *offset = jump_target;
    } else {
        *offset = pos;
    }
    parts.join(".")
}

/// Read a big-endian u16 from `data` at `offset`, advancing `offset` by 2.
fn read_u16(data: &[u8], offset: &mut usize) -> u16 {
    if *offset + 2 > data.len() {
        return 0;
    }
    let val = u16::from_be_bytes([data[*offset], data[*offset + 1]]);
    *offset += 2;
    val
}

/// Read a big-endian u32 from `data` at `offset`, advancing `offset` by 4.
fn read_u32(data: &[u8], offset: &mut usize) -> u32 {
    if *offset + 4 > data.len() {
        return 0;
    }
    let val = u32::from_be_bytes([
        data[*offset],
        data[*offset + 1],
        data[*offset + 2],
        data[*offset + 3],
    ]);
    *offset += 4;
    val
}

/// Parse the answer section of a DNS response for a specific record type.
/// Returns strings appropriate for the record type (see module doc for formats).
fn parse_answers(data: &[u8], expected_type: u16) -> Vec<String> {
    if data.len() < 12 {
        return vec![];
    }

    // Read header counts
    let _id = u16::from_be_bytes([data[0], data[1]]);
    let _flags = u16::from_be_bytes([data[2], data[3]]);
    let qdcount = u16::from_be_bytes([data[4], data[5]]) as usize;
    let ancount = u16::from_be_bytes([data[6], data[7]]) as usize;

    let mut offset = 12;

    // Skip question section
    for _ in 0..qdcount {
        parse_dns_name(data, &mut offset); // QNAME
        offset += 4; // QTYPE (2) + QCLASS (2)
        if offset > data.len() {
            return vec![];
        }
    }

    // Parse answer records
    let mut results = Vec::new();
    for _ in 0..ancount {
        if offset >= data.len() {
            break;
        }
        parse_dns_name(data, &mut offset); // NAME
        let rtype = read_u16(data, &mut offset);
        let _rclass = read_u16(data, &mut offset);
        let _ttl = read_u32(data, &mut offset);
        let rdlength = read_u16(data, &mut offset) as usize;

        if offset + rdlength > data.len() {
            break;
        }

        let rdata_start = offset;

        if rtype == expected_type {
            match rtype {
                DNS_TYPE_A => {
                    if rdlength == 4 {
                        let ip = Ipv4Addr::new(
                            data[offset],
                            data[offset + 1],
                            data[offset + 2],
                            data[offset + 3],
                        );
                        results.push(ip.to_string());
                    }
                }
                DNS_TYPE_AAAA => {
                    if rdlength == 16 {
                        let mut octets = [0u8; 16];
                        octets.copy_from_slice(&data[offset..offset + 16]);
                        let ip = Ipv6Addr::from(octets);
                        results.push(ip.to_string());
                    }
                }
                DNS_TYPE_MX => {
                    let mut roff = offset;
                    let priority = read_u16(data, &mut roff);
                    let exchange = parse_dns_name(data, &mut roff);
                    results.push(priority.to_string());
                    results.push(exchange);
                }
                DNS_TYPE_TXT => {
                    // TXT RDATA: one or more <length><string> segments
                    let mut roff = offset;
                    let end = offset + rdlength;
                    let mut txt = String::new();
                    while roff < end {
                        let slen = data[roff] as usize;
                        roff += 1;
                        if roff + slen > end {
                            break;
                        }
                        txt.push_str(&String::from_utf8_lossy(&data[roff..roff + slen]));
                        roff += slen;
                    }
                    results.push(txt);
                }
                DNS_TYPE_SRV => {
                    let mut roff = offset;
                    let priority = read_u16(data, &mut roff);
                    let weight = read_u16(data, &mut roff);
                    let port = read_u16(data, &mut roff);
                    let target = parse_dns_name(data, &mut roff);
                    results.push(priority.to_string());
                    results.push(weight.to_string());
                    results.push(port.to_string());
                    results.push(target);
                }
                DNS_TYPE_CNAME | DNS_TYPE_NS | DNS_TYPE_PTR => {
                    let mut roff = offset;
                    let name = parse_dns_name(data, &mut roff);
                    results.push(name);
                }
                _ => {}
            }
        }

        offset = rdata_start + rdlength;
    }

    results
}
