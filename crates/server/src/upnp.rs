//! Just enough UPnP (IGD) to ask a home router to forward the game port,
//! so friends can join over the internet without fiddling with router
//! settings. Plain std networking: SSDP discovery, one HTTP GET for the
//! device description, then SOAP calls.

use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, UdpSocket};
use std::time::{Duration, Instant};

const SERVICES: [&str; 3] = [
    "urn:schemas-upnp-org:service:WANIPConnection:2",
    "urn:schemas-upnp-org:service:WANIPConnection:1",
    "urn:schemas-upnp-org:service:WANPPPConnection:1",
];

pub struct Gateway {
    /// Router's control endpoint: host:port and path.
    host: SocketAddr,
    path: String,
    service: &'static str,
    /// Our LAN address as seen on the route to the router.
    pub local_ip: Ipv4Addr,
}

/// A port forwarded on the router; removed again when dropped.
pub struct Mapping {
    gw: Gateway,
    pub external_ip: Option<Ipv4Addr>,
    pub port: u16,
}

impl Drop for Mapping {
    fn drop(&mut self) {
        let body = format!(
            "<NewRemoteHost></NewRemoteHost><NewExternalPort>{}</NewExternalPort><NewProtocol>TCP</NewProtocol>",
            self.port
        );
        let _ = self.gw.soap("DeletePortMapping", &body);
    }
}

/// Find the router, forward TCP `port` to this machine and look up the
/// public address. Takes up to a few seconds; run it on a thread.
pub fn forward(port: u16, description: &str) -> Result<Mapping, String> {
    let gw = discover().ok_or("no UPnP router found (it may be switched off in the router's settings)")?;
    let mut last = String::new();
    // A lease means the port closes by itself even if we crash. Some routers
    // only accept unlimited (0) leases.
    for lease in [7200, 0] {
        let body = format!(
            "<NewRemoteHost></NewRemoteHost><NewExternalPort>{port}</NewExternalPort><NewProtocol>TCP</NewProtocol>\
             <NewInternalPort>{port}</NewInternalPort><NewInternalClient>{}</NewInternalClient><NewEnabled>1</NewEnabled>\
             <NewPortMappingDescription>{description}</NewPortMappingDescription><NewLeaseDuration>{lease}</NewLeaseDuration>",
            gw.local_ip
        );
        match gw.soap("AddPortMapping", &body) {
            Ok(_) => {
                let external_ip = gw
                    .soap("GetExternalIPAddress", "")
                    .ok()
                    .and_then(|r| tag(&r, "NewExternalIPAddress").and_then(|s| s.trim().parse().ok()));
                return Ok(Mapping { gw, external_ip, port });
            }
            Err(e) => last = e,
        }
    }
    Err(format!("the router refused to forward port {port} ({last})"))
}

/// Look for a UPnP router and ask its public address, changing nothing.
pub fn probe() -> Result<(Ipv4Addr, Option<Ipv4Addr>), String> {
    let gw = discover().ok_or("no UPnP router found")?;
    let ext = gw.soap("GetExternalIPAddress", "")?;
    Ok((gw.local_ip, tag(&ext, "NewExternalIPAddress").and_then(|s| s.trim().parse().ok())))
}

/// Is this an address the wider internet can't reach directly (the
/// router itself is behind another NAT, e.g. carrier-grade NAT)?
pub fn is_private(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    ip.is_private() || ip.is_loopback() || ip.is_link_local() || ip.is_unspecified() || (o[0] == 100 && (64..128).contains(&o[1]))
}

fn discover() -> Option<Gateway> {
    let sock = UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.set_read_timeout(Some(Duration::from_millis(300))).ok()?;
    for st in ["urn:schemas-upnp-org:device:InternetGatewayDevice:1", "urn:schemas-upnp-org:device:InternetGatewayDevice:2"] {
        let msg = format!("M-SEARCH * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nST: {st}\r\nMAN: \"ssdp:discover\"\r\nMX: 2\r\n\r\n");
        let _ = sock.send_to(msg.as_bytes(), "239.255.255.250:1900");
    }
    let deadline = Instant::now() + Duration::from_millis(2500);
    let mut buf = [0u8; 2048];
    let mut tried = Vec::new();
    while Instant::now() < deadline {
        let Ok((n, from)) = sock.recv_from(&mut buf) else { continue };
        let text = String::from_utf8_lossy(&buf[..n]);
        let Some(loc) = header(&text, "location") else { continue };
        if tried.contains(&loc) {
            continue;
        }
        tried.push(loc.clone());
        if let Some(gw) = describe(&loc, from.ip()) {
            return Some(gw);
        }
    }
    None
}

/// Fetch the device description and find a WAN connection service.
fn describe(location: &str, router: IpAddr) -> Option<Gateway> {
    let (host, path) = split_url(location)?;
    let xml = http(host, "GET", &path, &[], "").ok()?;
    for service in SERVICES {
        let Some(at) = xml.find(&format!("<serviceType>{service}</serviceType>")) else { continue };
        // The controlURL belongs to the same <service> block.
        let block_end = xml[at..].find("</service>").map_or(xml.len(), |e| at + e);
        let control = tag(&xml[at..block_end], "controlURL")?;
        let path = if control.starts_with("http") { split_url(&control)?.1 } else { control };
        let path = if path.starts_with('/') { path } else { format!("/{path}") };
        // Which of our addresses talks to the router.
        let probe = UdpSocket::bind("0.0.0.0:0").ok()?;
        probe.connect(SocketAddr::new(router, 1900)).ok()?;
        let IpAddr::V4(local_ip) = probe.local_addr().ok()?.ip() else { return None };
        return Some(Gateway { host, path, service, local_ip });
    }
    None
}

impl Gateway {
    fn soap(&self, action: &str, args: &str) -> Result<String, String> {
        let body = format!(
            "<?xml version=\"1.0\"?>\r\n<s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" \
             s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\"><s:Body>\
             <u:{action} xmlns:u=\"{svc}\">{args}</u:{action}></s:Body></s:Envelope>\r\n",
            svc = self.service
        );
        let soap_action = format!("\"{}#{action}\"", self.service);
        let reply = http(
            self.host,
            "POST",
            &self.path,
            &[("Content-Type", "text/xml; charset=\"utf-8\""), ("SOAPAction", &soap_action)],
            &body,
        )?;
        if reply.contains("Fault>") {
            let why = tag(&reply, "errorDescription").or_else(|| tag(&reply, "errorCode")).unwrap_or_else(|| "error".into());
            return Err(why);
        }
        Ok(reply)
    }
}

/// Minimal HTTP/1.1 request; returns the body (errors for non-2xx, except
/// SOAP faults, whose body the caller wants to read).
fn http(host: SocketAddr, method: &str, path: &str, headers: &[(&str, &str)], body: &str) -> Result<String, String> {
    let mut s = TcpStream::connect_timeout(&host, Duration::from_secs(3)).map_err(|e| e.to_string())?;
    s.set_read_timeout(Some(Duration::from_secs(4))).map_err(|e| e.to_string())?;
    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nContent-Length: {}\r\n", body.len());
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str("\r\n");
    req.push_str(body);
    s.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
    let mut raw = Vec::new();
    let _ = s.read_to_end(&mut raw);
    let text = String::from_utf8_lossy(&raw).into_owned();
    let (head, rest) = text.split_once("\r\n\r\n").ok_or("bad HTTP reply")?;
    let code: u32 = head.split_whitespace().nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
    let body = if header(head, "transfer-encoding").is_some_and(|v| v.eq_ignore_ascii_case("chunked")) {
        dechunk(rest)
    } else {
        rest.to_string()
    };
    if (200..300).contains(&code) || body.contains("Fault>") {
        Ok(body)
    } else {
        Err(format!("HTTP {code}"))
    }
}

fn dechunk(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some((len, after)) = rest.split_once("\r\n") {
        let Ok(n) = usize::from_str_radix(len.trim(), 16) else { break };
        if n == 0 || after.len() < n {
            break;
        }
        out.push_str(&after[..n]);
        rest = after[n..].trim_start_matches("\r\n");
    }
    out
}

fn header(text: &str, name: &str) -> Option<String> {
    text.lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.trim().eq_ignore_ascii_case(name).then(|| v.trim().to_string())
    })
}

/// Text inside the first `<name>...</name>` (ignoring any namespace prefix
/// on the element).
fn tag(xml: &str, name: &str) -> Option<String> {
    let mut from = 0;
    while let Some(i) = xml[from..].find(name) {
        let start = from + i;
        from = start + name.len();
        // Must be an opening tag: `<name>` or `<prefix:name>`.
        let before = xml[..start].rsplit('<').next().unwrap_or("");
        let opening = before.is_empty() || (before.ends_with(':') && !before.starts_with('/') && !before.contains(['>', ' ']));
        if !opening || !xml[from..].starts_with('>') || !xml[..start].contains('<') {
            continue;
        }
        let content = &xml[from + 1..];
        return Some(content[..content.find('<')?].trim().to_string());
    }
    None
}

fn split_url(url: &str) -> Option<(SocketAddr, String)> {
    let rest = url.strip_prefix("http://")?;
    let (hostport, path) = rest.split_once('/').map_or((rest, "/".to_string()), |(h, p)| (h, format!("/{p}")));
    let addr = if hostport.contains(':') { hostport.parse().ok()? } else { format!("{hostport}:80").parse().ok()? };
    Some((addr, path))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Talks to the real router (read-only): `cargo test -p server -- --ignored probe_router --nocapture`
    #[test]
    #[ignore]
    fn probe_router() {
        match probe() {
            Ok((local, ext)) => println!("router found; we are {local}, public address {ext:?}, behind another NAT: {}", ext.is_none_or(is_private)),
            Err(e) => println!("probe failed: {e}"),
        }
    }

    #[test]
    fn parses_bits() {
        assert_eq!(tag("<a><controlURL>/ctl/IPConn</controlURL></a>", "controlURL").as_deref(), Some("/ctl/IPConn"));
        assert_eq!(
            tag("<s:Body><u:R><NewExternalIPAddress>81.2.3.4</NewExternalIPAddress></u:R>", "NewExternalIPAddress").as_deref(),
            Some("81.2.3.4")
        );
        assert_eq!(header("HTTP/1.1 200 OK\r\nLOCATION: http://192.168.1.1:5000/rootDesc.xml\r\n", "location").as_deref(),
            Some("http://192.168.1.1:5000/rootDesc.xml"));
        let (h, p) = split_url("http://192.168.1.1:5000/rootDesc.xml").unwrap();
        assert_eq!((h.port(), p.as_str()), (5000, "/rootDesc.xml"));
        assert_eq!(dechunk("4\r\nabcd\r\n3\r\nefg\r\n0\r\n\r\n"), "abcdefg");
        assert!(is_private("100.72.1.1".parse().unwrap()) && is_private("192.168.0.5".parse().unwrap()));
        assert!(!is_private("81.2.3.4".parse().unwrap()));
    }
}
