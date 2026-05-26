use axum::http::HeaderMap;
use std::net::{IpAddr, SocketAddr};

const FLY_CLIENT_IP: &str = "fly-client-ip";
const X_FORWARDED_FOR: &str = "x-forwarded-for";

pub fn resolve_client_ip(headers: &HeaderMap, remote_addr: SocketAddr) -> IpAddr {
    header_ip(headers, FLY_CLIENT_IP)
        .or_else(|| forwarded_for_ip(headers))
        .unwrap_or_else(|| remote_addr.ip())
}

fn header_ip(headers: &HeaderMap, name: &str) -> Option<IpAddr> {
    headers.get(name)?.to_str().ok()?.trim().parse().ok()
}

fn forwarded_for_ip(headers: &HeaderMap) -> Option<IpAddr> {
    headers
        .get(X_FORWARDED_FOR)?
        .to_str()
        .ok()?
        .split(',')
        .next()?
        .trim()
        .parse()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_fly_client_ip() {
        let mut headers = HeaderMap::new();
        headers.insert(FLY_CLIENT_IP, "203.0.113.10".parse().unwrap());
        headers.insert(X_FORWARDED_FOR, "198.51.100.20".parse().unwrap());
        let remote = "10.0.0.1:1234".parse().unwrap();

        assert_eq!(
            resolve_client_ip(&headers, remote),
            "203.0.113.10".parse::<IpAddr>().unwrap()
        );
    }

    #[test]
    fn uses_first_forwarded_for_ip() {
        let mut headers = HeaderMap::new();
        headers.insert(
            X_FORWARDED_FOR,
            "198.51.100.20, 10.0.0.2".parse().unwrap(),
        );
        let remote = "10.0.0.1:1234".parse().unwrap();

        assert_eq!(
            resolve_client_ip(&headers, remote),
            "198.51.100.20".parse::<IpAddr>().unwrap()
        );
    }

    #[test]
    fn falls_back_to_socket_ip() {
        let headers = HeaderMap::new();
        let remote = "10.0.0.1:1234".parse().unwrap();

        assert_eq!(
            resolve_client_ip(&headers, remote),
            "10.0.0.1".parse::<IpAddr>().unwrap()
        );
    }
}
