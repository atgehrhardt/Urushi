//! Which addresses LAN access may listen on and accept connections from.
//!
//! Only private addresses: IPv4's RFC 1918 ranges and IPv6 unique local addresses. Never
//! a public address, never every interface (`0.0.0.0`, `::`), and never Tailscale's own
//! ranges, which tailnet serving covers with its own identity checks (ADR 0005).

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Whether `ip` is a private LAN address LAN access may use.
pub fn is_lan_address(ip: IpAddr) -> bool {
    match canonical(ip) {
        IpAddr::V4(v4) => is_private_v4(v4),
        IpAddr::V6(v6) => is_unique_local_v6(v6) && !is_tailscale_v6(v6),
    }
}

/// Whether a connection from `ip` may even start a LAN handshake: a LAN address, or this
/// machine itself. Neither is enough on its own; the device must still be paired.
pub fn is_allowed_peer(ip: IpAddr) -> bool {
    is_lan_address(ip) || canonical(ip).is_loopback()
}

/// IPv4-mapped IPv6 addresses (`::ffff:192.168.1.2`) as the IPv4 address they are.
fn canonical(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(ip, IpAddr::V4),
        IpAddr::V4(_) => ip,
    }
}

fn is_private_v4(ip: Ipv4Addr) -> bool {
    // 10/8, 172.16/12 and 192.168/16. Tailscale's 100.64/10 is not among them.
    ip.is_private()
}

fn is_unique_local_v6(ip: Ipv6Addr) -> bool {
    // fc00::/7
    (ip.segments()[0] & 0xfe00) == 0xfc00
}

fn is_tailscale_v6(ip: Ipv6Addr) -> bool {
    // fd7a:115c:a1e0::/48, Tailscale's own unique local range.
    let s = ip.segments();
    s[0] == 0xfd7a && s[1] == 0x115c && s[2] == 0xa1e0
}

/// This machine's addresses LAN access may listen on.
pub trait LanInterfaces: Send + Sync {
    /// The addresses to listen on. Implementations return only addresses that are safe
    /// to listen on; the listener checks again anyway.
    fn addresses(&self) -> std::io::Result<Vec<IpAddr>>;
}

/// The network interfaces' private addresses, read from the OS.
#[derive(Debug, Default)]
pub struct SystemInterfaces;

impl LanInterfaces for SystemInterfaces {
    fn addresses(&self) -> std::io::Result<Vec<IpAddr>> {
        let mut addresses: Vec<IpAddr> = if_addrs::get_if_addrs()?
            .into_iter()
            .map(|interface| interface.ip())
            .filter(|ip| is_lan_address(*ip))
            .collect();
        addresses.sort();
        addresses.dedup();
        Ok(addresses)
    }
}

/// Fixed addresses, for tests (which listen on loopback).
#[derive(Debug, Default)]
pub struct StaticInterfaces(pub Vec<IpAddr>);

impl LanInterfaces for StaticInterfaces {
    fn addresses(&self) -> std::io::Result<Vec<IpAddr>> {
        Ok(self.0.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(text: &str) -> IpAddr {
        text.parse().unwrap()
    }

    #[test]
    fn private_ranges_are_lan() {
        for text in [
            "10.0.0.1",
            "172.16.4.2",
            "172.31.255.254",
            "192.168.1.10",
            "fd12:3456:789a::1",
            "fc00::1",
            "::ffff:192.168.1.10",
        ] {
            assert!(is_lan_address(ip(text)), "{text}");
            assert!(is_allowed_peer(ip(text)), "{text}");
        }
    }

    #[test]
    fn public_tailnet_and_special_addresses_are_not() {
        for text in [
            "0.0.0.0",
            "::",
            "8.8.8.8",
            "172.32.0.1",
            "100.64.0.1",
            "100.101.102.103",
            "fd7a:115c:a1e0::1",
            "169.254.1.1",
            "fe80::1",
            "2001:db8::1",
            "127.0.0.1",
            "::1",
            "255.255.255.255",
            "224.0.0.1",
        ] {
            assert!(!is_lan_address(ip(text)), "{text}");
        }
    }

    #[test]
    fn loopback_may_connect_but_is_not_listened_on() {
        assert!(is_allowed_peer(ip("127.0.0.1")));
        assert!(is_allowed_peer(ip("::1")));
        assert!(!is_allowed_peer(ip("8.8.8.8")));
        assert!(!is_allowed_peer(ip("100.64.0.1")));
    }

    #[test]
    fn the_system_lists_only_lan_addresses() {
        for address in SystemInterfaces.addresses().unwrap() {
            assert!(is_lan_address(address), "{address}");
        }
    }
}
