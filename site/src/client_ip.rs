use axum::http::HeaderMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

const FLY_CLIENT_IP: &str = "fly-client-ip";

/// Return the real client IP.
///
/// `fly-client-ip` is only trusted when the TCP connection itself arrived from
/// a private/internal address, which means it was forwarded by Fly's proxy.
/// A direct public connection cannot forge a private source IP at the TCP
/// layer, so this prevents header-spoofing attacks that would bypass per-IP
/// rate limiting.
pub fn resolve_client_ip(headers: &HeaderMap, remote_addr: SocketAddr) -> IpAddr {
    let socket_ip = remote_addr.ip();
    if is_private_ip(&socket_ip) {
        header_ip(headers, FLY_CLIENT_IP).unwrap_or(socket_ip)
    } else {
        socket_ip
    }
}

fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_private_v4(v4),
        IpAddr::V6(v6) => is_private_v6(v6),
    }
}

fn is_private_v4(ip: &Ipv4Addr) -> bool {
    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_unspecified()
}

fn is_private_v6(ip: &Ipv6Addr) -> bool {
    ip.is_loopback()
        || ip.is_unspecified()
        // Fly internal network: fdaa::/8 (ULA fd00::/8)
        || ip.octets()[0] == 0xfd
        // IPv4-mapped private: ::ffff:10.x, ::ffff:172.16-31.x, ::ffff:192.168.x
        || matches!(ip.to_ipv4_mapped(), Some(v4) if is_private_v4(&v4))
}

fn header_ip(headers: &HeaderMap, name: &str) -> Option<IpAddr> {
    headers.get(name)?.to_str().ok()?.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_fly_client_ip_when_connection_is_from_private_ip() {
        let mut headers = HeaderMap::new();
        headers.insert(FLY_CLIENT_IP, "203.0.113.10".parse().unwrap());
        // TCP connection from Fly's internal network
        let remote = "10.0.0.1:1234".parse().unwrap();

        assert_eq!(
            resolve_client_ip(&headers, remote),
            "203.0.113.10".parse::<IpAddr>().unwrap()
        );
    }

    #[test]
    fn ignores_fly_client_ip_when_connection_is_from_public_ip() {
        let mut headers = HeaderMap::new();
        headers.insert(FLY_CLIENT_IP, "1.2.3.4".parse().unwrap());
        // Direct public TCP connection — header cannot be trusted
        let remote = "203.0.113.99:1234".parse().unwrap();

        assert_eq!(
            resolve_client_ip(&headers, remote),
            "203.0.113.99".parse::<IpAddr>().unwrap()
        );
    }

    #[test]
    fn ignores_forwarded_for_ip() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            "198.51.100.20, 10.0.0.2".parse().unwrap(),
        );
        let remote = "10.0.0.1:1234".parse().unwrap();

        assert_eq!(
            resolve_client_ip(&headers, remote),
            "10.0.0.1".parse::<IpAddr>().unwrap()
        );
    }

    #[test]
    fn falls_back_to_socket_ip_when_no_header() {
        let headers = HeaderMap::new();
        let remote = "10.0.0.1:1234".parse().unwrap();

        assert_eq!(
            resolve_client_ip(&headers, remote),
            "10.0.0.1".parse::<IpAddr>().unwrap()
        );
    }
}
