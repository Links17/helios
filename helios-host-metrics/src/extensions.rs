use reqwest::Client;
use std::net::IpAddr;
use std::str::FromStr;
use tracing::warn;

use crate::{PublicIpConfig, ReservedHostMetrics, ReservedMetricsConfig};

#[derive(Debug, Clone)]
pub struct ReservedMetricsEnricher {
    config: ReservedMetricsConfig,
    client: Client,
}

impl ReservedMetricsEnricher {
    pub fn new(config: ReservedMetricsConfig) -> Self {
        Self {
            config,
            client: Client::new(),
        }
    }

    pub async fn enrich(&self, reserved: &mut ReservedHostMetrics) {
        if let Some(public_ip) = self.fetch_public_ip().await {
            reserved.public_ip = vec![public_ip];
        }
    }

    pub fn cpu_temperature_enabled(&self) -> bool {
        self.config.cpu_temperature_enable
    }

    async fn fetch_public_ip(&self) -> Option<String> {
        let PublicIpConfig { endpoint, timeout } = &self.config.public_ip;
        let endpoint = endpoint.as_ref()?;

        match self
            .client
            .get(endpoint)
            .timeout(*timeout)
            .send()
            .await
            .and_then(|response| response.error_for_status())
        {
            Ok(response) => match response.text().await {
                Ok(body) => normalize_public_ip_response(&body),
                Err(error) => {
                    warn!(?error, "failed to read public ip response body");
                    None
                }
            },
            Err(error) => {
                warn!(?error, endpoint, "failed to fetch public ip");
                None
            }
        }
    }
}

impl Default for ReservedMetricsEnricher {
    fn default() -> Self {
        Self::new(ReservedMetricsConfig::default())
    }
}

fn normalize_public_ip_response(body: &str) -> Option<String> {
    let candidate = body.lines().next()?.trim();
    let ip = IpAddr::from_str(candidate).ok()?;
    Some(ip.to_string())
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;
    use std::time::Duration;

    use super::*;

    #[test]
    fn public_ip_response_parser_accepts_plain_ip() {
        assert_eq!(
            normalize_public_ip_response("1.2.3.4\n"),
            Some("1.2.3.4".to_string())
        );
    }

    #[test]
    fn public_ip_response_parser_rejects_invalid_payload() {
        assert_eq!(normalize_public_ip_response("not-an-ip"), None);
    }

    #[tokio::test]
    async fn it_enriches_public_ip_when_endpoint_is_configured() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();

        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buffer = [0_u8; 1024];
            let _ = stream.read(&mut buffer);
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\n\r\n1.2.3.4\n")
                .unwrap();
        });

        let enricher = ReservedMetricsEnricher::new(ReservedMetricsConfig {
            cpu_temperature_enable: true,
            public_ip: PublicIpConfig {
                endpoint: Some(format!("http://{address}")),
                timeout: Duration::from_secs(1),
            },
        });
        let mut reserved = ReservedHostMetrics::default();

        enricher.enrich(&mut reserved).await;

        server.join().unwrap();
        assert_eq!(reserved.public_ip, vec!["1.2.3.4".to_string()]);
    }
}
