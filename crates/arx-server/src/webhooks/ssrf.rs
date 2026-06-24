//! SSRF guard for outbound webhook delivery.
//!
//! Scheme-only validation is not SSRF protection: the daemon shares a host with
//! the loopback-only admin API (`127.0.0.1:7878`) and can usually reach cloud
//! metadata (`169.254.169.254`). We therefore vet the *resolved* IP at dial time
//! via a custom [`reqwest::dns::Resolve`] implementation that filters disallowed
//! addresses inside the resolver itself — there is no separate "validate then
//! connect" step, which closes the DNS-rebinding / happy-eyeballs TOCTOU window.
//!
//! Policy (self-hosted home server): general private LAN is allowed (sending to
//! another box on your own network is a legitimate core use case), but loopback,
//! link-local, cloud metadata, and unspecified/special ranges are always blocked
//! because reaching those is a confused-deputy attack surface, not a use case.

use hickory_resolver::TokioAsyncResolver;
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;

/// Returns true if an IP must never be a webhook target.
///
/// Blocks loopback, link-local (incl. cloud metadata 169.254.169.254), the
/// unspecified address, IPv4-mapped/compatible and transition ranges that can
/// smuggle a blocked v4 address (NAT64 64:ff9b::/96, 6to4 2002::/16, Teredo
/// 2001::/32), and the AWS IMDS IPv6 form fd00:ec2::254. General ULA/private
/// LAN is intentionally allowed.
pub fn is_blocked_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_blocked_v4(v4),
        IpAddr::V6(v6) => {
            // Unwrap IPv4-mapped / IPv4-compatible and re-check as v4.
            if let Some(v4) = v6.to_ipv4() {
                return is_blocked_v4(v4);
            }
            is_blocked_v6(v6)
        }
    }
}

fn is_blocked_v4(ip: Ipv4Addr) -> bool {
    ip.is_loopback()            // 127.0.0.0/8
        || ip.is_link_local()   // 169.254.0.0/16 (includes 169.254.169.254 metadata)
        || ip.is_unspecified()  // 0.0.0.0
        || ip.is_broadcast()    // 255.255.255.255
        || ip.is_documentation()
        // 0.0.0.0/8 "this network"
        || ip.octets()[0] == 0
}

fn is_blocked_v6(ip: Ipv6Addr) -> bool {
    if ip.is_loopback() || ip.is_unspecified() {
        return true;
    }
    let seg = ip.segments();
    // Link-local fe80::/10
    if (seg[0] & 0xffc0) == 0xfe80 {
        return true;
    }
    // NAT64 well-known prefix 64:ff9b::/96 — can encapsulate v4 metadata.
    if seg[0] == 0x0064 && seg[1] == 0xff9b {
        return true;
    }
    // 6to4 2002::/16 — embeds a v4 address that may be private/link-local.
    if seg[0] == 0x2002 {
        return true;
    }
    // Teredo 2001::/32.
    if seg[0] == 0x2001 && seg[1] == 0x0000 {
        return true;
    }
    // AWS IMDS over IPv6: fd00:ec2::254 (within ULA, special-cased as metadata).
    if seg[0] == 0xfd00 && seg[1] == 0x0ec2 {
        return true;
    }
    false
}

/// Validates a parsed URL's scheme up front. Host/IP vetting happens at dial
/// time in [`GuardedResolver`]; if the host is already an IP literal we can
/// reject it here too.
pub fn validate_url(url: &str) -> Result<(), String> {
    let parsed = reqwest::Url::parse(url).map_err(|e| format!("invalid url: {e}"))?;
    match parsed.scheme() {
        "http" | "https" => {}
        other => return Err(format!("unsupported scheme `{other}` (use http or https)")),
    }
    if parsed.host().is_none() {
        return Err("url has no host".into());
    }
    // Reject IP-literal hosts that are already blocked (cheap early check; the
    // resolver still vets DNS-resolved hosts).
    if let Some(host) = parsed.host_str() {
        if let Ok(ip) = host.parse::<IpAddr>() {
            if is_blocked_ip(ip) {
                return Err("url points at a blocked address".into());
            }
        }
    }
    Ok(())
}

/// A [`reqwest::dns::Resolve`] that resolves via the system/Google resolver and
/// drops any blocked address from the returned set. reqwest keeps the original
/// hostname for SNI/cert validation; only the dialled socket addresses are
/// filtered, so TLS verification is unaffected.
#[derive(Clone)]
pub struct GuardedResolver {
    inner: Arc<TokioAsyncResolver>,
}

impl GuardedResolver {
    pub fn new() -> Self {
        // System resolver config; falls back to a sane default if unavailable.
        let resolver = TokioAsyncResolver::tokio_from_system_conf()
            .unwrap_or_else(|_| TokioAsyncResolver::tokio(Default::default(), Default::default()));
        Self {
            inner: Arc::new(resolver),
        }
    }
}

impl Resolve for GuardedResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let resolver = self.inner.clone();
        Box::pin(async move {
            let host = name.as_str().to_string();
            let lookup = resolver
                .lookup_ip(host.as_str())
                .await
                .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { Box::new(e) })?;
            let allowed: Vec<SocketAddr> = lookup
                .iter()
                .filter(|ip| !is_blocked_ip(*ip))
                .map(|ip| SocketAddr::new(ip, 0))
                .collect();
            if allowed.is_empty() {
                return Err(Box::<dyn std::error::Error + Send + Sync>::from(format!(
                    "all resolved addresses for `{host}` are blocked"
                )));
            }
            let addrs: Addrs = Box::new(allowed.into_iter());
            Ok(addrs)
        })
    }
}

impl Default for GuardedResolver {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_loopback_v4() {
        assert!(is_blocked_ip("127.0.0.1".parse().unwrap()));
        assert!(is_blocked_ip("127.5.6.7".parse().unwrap()));
    }

    #[test]
    fn blocks_metadata_and_link_local() {
        assert!(is_blocked_ip("169.254.169.254".parse().unwrap()));
        assert!(is_blocked_ip("169.254.0.1".parse().unwrap()));
    }

    #[test]
    fn blocks_unspecified_and_this_network() {
        assert!(is_blocked_ip("0.0.0.0".parse().unwrap()));
        assert!(is_blocked_ip("0.1.2.3".parse().unwrap()));
    }

    #[test]
    fn blocks_loopback_v6_and_mapped() {
        assert!(is_blocked_ip("::1".parse().unwrap()));
        assert!(is_blocked_ip("::ffff:127.0.0.1".parse().unwrap()));
        assert!(is_blocked_ip("::ffff:169.254.169.254".parse().unwrap()));
    }

    #[test]
    fn blocks_v6_transition_and_metadata() {
        assert!(is_blocked_ip("fe80::1".parse().unwrap()));
        assert!(is_blocked_ip("64:ff9b::a9fe:a9fe".parse().unwrap()));
        assert!(is_blocked_ip("2002::1".parse().unwrap()));
        assert!(is_blocked_ip("2001::1".parse().unwrap()));
        assert!(is_blocked_ip("fd00:ec2::254".parse().unwrap()));
    }

    #[test]
    fn allows_public_and_private_lan() {
        // Public.
        assert!(!is_blocked_ip("1.1.1.1".parse().unwrap()));
        assert!(!is_blocked_ip("93.184.216.34".parse().unwrap()));
        // Private LAN is allowed (self-hosted home-server use case).
        assert!(!is_blocked_ip("192.168.1.50".parse().unwrap()));
        assert!(!is_blocked_ip("10.0.0.5".parse().unwrap()));
        assert!(!is_blocked_ip("172.16.0.9".parse().unwrap()));
        // General ULA (non-metadata) allowed.
        assert!(!is_blocked_ip("fd12:3456::1".parse().unwrap()));
    }

    #[test]
    fn validate_url_rejects_bad_scheme_and_ip_literals() {
        assert!(validate_url("ftp://example.com").is_err());
        assert!(validate_url("http://127.0.0.1:7878/x").is_err());
        assert!(validate_url("https://169.254.169.254/").is_err());
        assert!(validate_url("https://example.com/hook").is_ok());
        assert!(validate_url("http://192.168.1.10/hook").is_ok());
    }
}
