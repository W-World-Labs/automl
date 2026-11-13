use reqwest::StatusCode;
use serde_json::{json, Value};

fn base_url() -> Option<String> {
    std::env::var("AUTOML_TEST_BASE_URL")
        .ok()
        .map(|url| url.trim_end_matches('/').to_string())
        .filter(|url| !url.is_empty())
}

#[tokio::test]
async fn live_agent_contract_exposes_public_onboarding_and_protects_private_key_routes(
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(base) = base_url() else {
        eprintln!("skipping live contract test: AUTOML_TEST_BASE_URL is not set");
        return Ok(());
    };
    let client = reqwest::Client::new();

    let health: Value = client
        .get(format!("{base}/api/v1/health"))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(health["status"], "ok");
    assert_eq!(health["services"]["database"], "connected");
    assert_eq!(health["services"]["obscuraid"], "configured");
    assert_eq!(health["services"]["radar"], "configured");

    let onboarding: Value = client
        .get(format!("{base}/api/v1/onboarding"))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert!(onboarding["principle"]
        .as_str()
        .unwrap_or_default()
        .contains("Bring your own model"));

    let unauth_scholar_status = client
        .post(format!("{base}/api/v1/scholars"))
        .json(&json!({
            "name": "AutoML Live Contract Scholar"
        }))
        .send()
        .await?
        .status();
    assert_eq!(unauth_scholar_status, StatusCode::UNAUTHORIZED);

    let unauth_device_status = client
        .post(format!("{base}/api/v1/agents/bind-device"))
        .json(&json!({
            "enrollment_token": "enroll_invalid",
            "device_fingerprint": "codex-live-contract-device",
            "hostname": "codex-live-contract"
        }))
        .send()
        .await?
        .status();
    assert_eq!(unauth_device_status, StatusCode::UNAUTHORIZED);

    let unauth_key_status = client
        .post(format!("{base}/api/v1/auth/keys"))
        .json(&json!({ "name": "unauthenticated contract test" }))
        .send()
        .await?
        .status();
    assert_eq!(unauth_key_status, StatusCode::UNAUTHORIZED);

    Ok(())
}
