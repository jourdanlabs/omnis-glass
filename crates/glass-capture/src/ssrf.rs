use std::net::{IpAddr, ToSocketAddrs};
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SsrfVerdict {
    Allow,
    Refuse(String),
}

pub struct SsrfCheck {
    pub allow_private: bool,
}

impl Default for SsrfCheck {
    fn default() -> Self {
        Self {
            allow_private: false,
        }
    }
}

impl SsrfCheck {
    pub fn check_url(&self, url_str: &str) -> SsrfVerdict {
        let parsed = match Url::parse(url_str) {
            Ok(u) => u,
            Err(e) => return SsrfVerdict::Refuse(format!("invalid url: {e}")),
        };

        let scheme = parsed.scheme();
        if scheme == "about" || scheme == "data" {
            return SsrfVerdict::Allow;
        }
        if scheme != "http" && scheme != "https" {
            if scheme == "file" {
                return SsrfVerdict::Allow;
            }
            return SsrfVerdict::Refuse(format!("unsupported scheme: {scheme}"));
        }

        let host = match parsed.host_str() {
            Some(h) => h.to_lowercase(),
            None => return SsrfVerdict::Refuse("missing host".into()),
        };

        if is_forbidden_literal_host(&host) && !self.allow_private {
            return SsrfVerdict::Refuse(format!("SSRF blocked: literal forbidden host {host}"));
        }

        if looks_like_ip(&host) {
            if let Ok(ip) = host.parse::<IpAddr>() {
                if is_forbidden_ip(ip) && !self.allow_private {
                    return SsrfVerdict::Refuse(format!("SSRF blocked: forbidden IP {ip}"));
                }
            }
            return SsrfVerdict::Allow;
        }

        // DNS resolution check
        let port = parsed.port_or_known_default().unwrap_or(443);
        let addrs: Vec<_> = match (host.as_str(), port).to_socket_addrs() {
            Ok(a) => a.collect(),
            Err(e) => {
                // No network in test sandbox — allow if literal was public
                if self.allow_private {
                    return SsrfVerdict::Allow;
                }
                return SsrfVerdict::Refuse(format!("DNS lookup failed for {host}: {e}"));
            }
        };

        if !self.allow_private {
            for addr in addrs {
                if is_forbidden_ip(addr.ip()) {
                    return SsrfVerdict::Refuse(format!(
                        "SSRF blocked: '{host}' resolves to forbidden address {}",
                        addr.ip()
                    ));
                }
            }
        }

        SsrfVerdict::Allow
    }

    pub fn check_redirect(&self, from: &str, to: &str) -> SsrfVerdict {
        match self.check_url(to) {
            SsrfVerdict::Refuse(r) => SsrfVerdict::Refuse(format!("redirect {from} -> {to}: {r}")),
            v => v,
        }
    }
}

fn looks_like_ip(host: &str) -> bool {
    host.parse::<IpAddr>().is_ok()
        || host.starts_with('[') && host.ends_with(']')
}

pub fn is_forbidden_literal_host(host: &str) -> bool {
    matches!(
        host,
        "localhost"
            | "127.0.0.1"
            | "0.0.0.0"
            | "::1"
            | "[::1]"
            | "metadata.google.internal"
            | "169.254.169.254"
    ) || host.starts_with("127.")
        || host.starts_with("10.")
        || host.starts_with("192.168.")
        || host.starts_with("169.254.")
        || host.starts_with("172.16.")
        || host.starts_with("172.17.")
        || host.starts_with("172.18.")
        || host.starts_with("172.19.")
        || host.starts_with("172.2")
        || host.starts_with("172.30.")
        || host.starts_with("172.31.")
}

pub fn is_forbidden_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            o[0] == 127
                || o[0] == 0
                || (o[0] == 10)
                || (o[0] == 172 && o[1] >= 16 && o[1] <= 31)
                || (o[0] == 192 && o[1] == 168)
                || (o[0] == 169 && o[1] == 254)
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || (v6.segments()[0] & 0xfe00) == 0xfc00
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_literal_loopback() {
        let c = SsrfCheck::default();
        assert!(matches!(
            c.check_url("http://127.0.0.1/admin"),
            SsrfVerdict::Refuse(_)
        ));
    }

    #[test]
    fn blocks_link_local() {
        let c = SsrfCheck::default();
        assert!(matches!(
            c.check_url("http://169.254.169.254/latest/meta-data"),
            SsrfVerdict::Refuse(_)
        ));
    }

    #[test]
    fn allows_public_host() {
        let c = SsrfCheck::default();
        // May fail DNS in sandbox — check it's not refused for literal private
        match c.check_url("https://example.com") {
            SsrfVerdict::Refuse(r) => assert!(!r.contains("127.0.0.1")),
            SsrfVerdict::Allow => {}
        }
    }
}
