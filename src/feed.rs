//! SimpleFIN Bridge: the only network code. Parsing and posting live in `Books::import_feed`.

use base64::{engine::general_purpose::STANDARD, Engine};

/// Uses the OS certificate store, so antivirus HTTPS scanning and corporate roots still verify.
fn agent() -> ureq::Agent {
    let tls = ureq::tls::TlsConfig::builder().root_certs(ureq::tls::RootCerts::PlatformVerifier).build();
    ureq::Agent::config_builder().tls_config(tls).build().new_agent()
}

/// Exchanges a one-time setup token for the long-lived access URL (it holds credentials; store it like a password).
pub fn claim(setup_token: &str) -> Result<String, String> {
    let claim_url = STANDARD
        .decode(setup_token.trim())
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
        .filter(|u| u.starts_with("https://"))
        .ok_or("that doesn't look like a SimpleFIN setup token")?;
    let access = agent()
        .post(&claim_url)
        .send_empty()
        .map_err(|e| format!("claiming the token failed (tokens work only once): {e}"))?
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())?;
    let access = access.trim().to_string();
    if !access.starts_with("https://") {
        return Err("SimpleFIN returned an unexpected reply".into());
    }
    Ok(access)
}

/// Downloads accounts and transactions posted since `start` (unix seconds) as raw JSON.
pub fn fetch(access_url: &str, start: i64) -> Result<String, String> {
    // The access URL is https://user:pass@host/path; send the credentials as a Basic auth header.
    let (creds, host_path) = access_url
        .strip_prefix("https://")
        .and_then(|r| r.split_once('@'))
        .ok_or("stored SimpleFIN access URL is malformed; connect again")?;
    agent()
        .get(format!("https://{host_path}/accounts?start-date={start}"))
        .header("Authorization", format!("Basic {}", STANDARD.encode(creds)))
        .call()
        .map_err(|e| format!("download failed: {e}"))?
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    /// Hits SimpleFIN's public demo account (fake data). Run with: cargo test -- --ignored simplefin
    #[test]
    #[ignore]
    fn simplefin_demo() {
        let token = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, "https://beta-bridge.simplefin.org/simplefin/claim/DEMO");
        let url = super::claim(&token).unwrap_or_else(|e| {
            eprintln!("claim failed ({e}); using the published demo access URL");
            "https://demo:demo@beta-bridge.simplefin.org/simplefin".into()
        });
        let start = chrono::Utc::now().timestamp() - 60 * 86_400;
        let json = super::fetch(&url, start).unwrap();
        let b = crate::books::Books::open(":memory:").unwrap();
        assert_eq!(b.import_feed(&json), Ok((0, 0)), "first pass only discovers accounts");
        let accts = b.feed_accounts().unwrap();
        eprintln!("accounts: {:?}", accts.iter().map(|a| &a.1).collect::<Vec<_>>());
        let chk = b.accounts().unwrap()[0].id;
        for a in &accts {
            b.map_feed_account(&a.0, chk).unwrap();
        }
        let (n, s) = b.import_feed(&json).unwrap();
        eprintln!("imported {n}, skipped {s}");
        assert!(n > 0, "demo has transactions");
        assert_eq!(b.import_feed(&json).unwrap(), (0, n), "second download posts nothing new");
    }
}
