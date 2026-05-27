use axum::http::HeaderMap;
use std::net::{IpAddr, SocketAddr};

const FLY_CLIENT_IP: &str = "fly-client-ip";

pub fn resolve_client_ip(headers: &HeaderMap, remote_addr: SocketAddr) -> IpAddr {
    header_ip(headers, FLY_CLIENT_IP).unwrap_or_else(|| remote_addr.ip())
}

fn header_ip(headers: &HeaderMap, name: &str) -> Option<IpAddr> {
    headers.get(name)?.to_str().ok()?.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_fly_client_ip() {
        let mut headers = HeaderMap::new();
        headers.insert(FLY_CLIENT_IP, "203.0.113.10".parse().unwrap());
        let remote = "10.0.0.1:1234".parse().unwrap();

        assert_eq!(
            resolve_client_ip(&headers, remote),
            "203.0.113.10".parse::<IpAddr>().unwrap()
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
    fn falls_back_to_socket_ip() {
        let headers = HeaderMap::new();
        let remote = "10.0.0.1:1234".parse().unwrap();

        assert_eq!(
            resolve_client_ip(&headers, remote),
            "10.0.0.1".parse::<IpAddr>().unwrap()
        );
    }
}
