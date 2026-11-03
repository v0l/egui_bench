use std::sync::{Arc, OnceLock};
use std::time::Duration;

/// Addresses web tools must not fetch: the cloud-metadata range and the unspecified
/// address. Link-local (169.254.0.0/16, fe80::/10) is where cloud metadata endpoints
/// live (169.254.169.254), and 0.0.0.0/:: are never real fetch targets.
pub fn refused_target(url: &url::Url) -> Option<String> {
    let host = url.host_str()?.to_ascii_lowercase();
    if host == "metadata.google.internal" || host == "metadata.goog" {
        return Some(host);
    }
    match url.host() {
        Some(url::Host::Ipv4(ip)) => {
            let [a, b, _, _] = ip.octets();
            if ip.is_unspecified() || (a == 169 && b == 254) {
                Some(ip.to_string())
            } else {
                None
            }
        }
        Some(url::Host::Ipv6(ip)) => {
            let seg = ip.segments();
            let link_local = (seg[0] & 0xffc0) == 0xfe80;
            if ip.is_unspecified() || link_local {
                Some(ip.to_string())
            } else {
                None
            }
        }
        _ => None,
    }
}

fn tls() -> rustls::ClientConfig {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let roots = rustls::RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };
    rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("the ring provider supports the default TLS versions")
        .with_root_certificates(roots)
        .with_no_client_auth()
}

pub fn builder() -> reqwest::ClientBuilder {
    // Every hop of a redirect is checked, so a public-looking address cannot bounce the
    // client into a metadata endpoint.
    let b = reqwest::Client::builder().redirect(reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() >= 10 {
            return attempt.error("too many redirects");
        }
        if let Some(bad) = refused_target(attempt.url()) {
            return attempt.error(format!("not following the redirect to {bad}"));
        }
        attempt.follow()
    }))
        .tls_backend_preconfigured(tls())
        .user_agent(format!("egui_bench/{}", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(10))
        .pool_idle_timeout(Duration::from_secs(30))
        .tcp_keepalive(Duration::from_secs(15));
    #[cfg(target_os = "linux")]
    let b = b.tcp_user_timeout(Duration::from_secs(30));
    b
}

pub fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| builder().build().expect("an http client"))
}

pub fn describe(e: &reqwest::Error) -> String {
    let mut out = e.to_string();
    let mut source = std::error::Error::source(e);
    while let Some(cause) = source {
        let text = cause.to_string();
        if !out.contains(&text) {
            out.push_str(": ");
            out.push_str(&text);
        }
        source = cause.source();
    }
    out
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    #[ignore = "reaches the internet"]
    async fn https_works_with_the_bundled_roots() {
        let r = super::client().get("https://example.com/").send().await.unwrap();
        assert!(r.status().as_u16() < 500, "{}", r.status());
    }
}
