use hickory_resolver::TokioAsyncResolver;
use hickory_resolver::config::{NameServerConfigGroup, ResolverConfig, ResolverOpts};
use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum DnsError {
    #[error("hostname has no A record (or lookup failed): {0}")]
    NoRecord(String),

    #[error("hostname `{host}` resolves to {found:?}, expected {expected}")]
    Mismatch {
        host: String,
        found: Vec<IpAddr>,
        expected: IpAddr,
    },

    #[error("dns lookup error: {0}")]
    Lookup(String),
}

fn build_resolver() -> TokioAsyncResolver {
    let cfg = ResolverConfig::from_parts(
        None,
        vec![],
        NameServerConfigGroup::from_ips_clear(&[IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))], 53, true),
    );
    let mut opts = ResolverOpts::default();
    opts.timeout = Duration::from_secs(3);
    opts.attempts = 1;
    TokioAsyncResolver::tokio(cfg, opts)
}

pub async fn verify_a_record(hostname: &str, expected_ip: IpAddr) -> Result<(), DnsError> {
    let resolver = build_resolver();
    let lookup = resolver
        .ipv4_lookup(hostname)
        .await
        .map_err(|e| DnsError::Lookup(e.to_string()))?;

    let found: Vec<IpAddr> = lookup.iter().map(|r| IpAddr::V4(r.0)).collect();

    if found.is_empty() {
        return Err(DnsError::NoRecord(hostname.to_string()));
    }
    if !found.iter().any(|ip| ip == &expected_ip) {
        return Err(DnsError::Mismatch {
            host: hostname.to_string(),
            found,
            expected: expected_ip,
        });
    }
    Ok(())
}
