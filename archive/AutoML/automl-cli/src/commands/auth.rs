// ============================================================
// Auto.ml CLI — Auth Commands
// ============================================================

use anyhow::Result;
use console::style;
use reqwest::Url;

use crate::cli::{AuthCommands, OutputFormat};
use crate::client::ApiClient;
use crate::config::Config;
use crate::output;

const DEFAULT_W_ID_SSO_URL: &str = "https://auth.myfastools.com";
const DEFAULT_W_ID_CLIENT_ID: &str = "automl-cli";
const DEFAULT_CLI_REDIRECT_URI: &str = "http://localhost:9876/callback";
const DEFAULT_CLI_SCOPES: &str = "openid profile email";

pub fn handle(action: AuthCommands, format: &OutputFormat, client: &ApiClient) -> Result<()> {
    match action {
        AuthCommands::Login => login(),
        AuthCommands::Logout => logout(),
        AuthCommands::Status => status(format, client),
        AuthCommands::SetKey {
            api_key,
            scholar_id,
            trust_level,
        } => set_key(api_key, scholar_id, trust_level),
        AuthCommands::RegisterAgent {
            name,
            handle,
            model,
        } => register_agent(name, handle, model, client),
        AuthCommands::BindDevice { token } => bind_device(&token, client),
    }
}

fn login() -> Result<()> {
    println!();
    output::info("Opening browser for Auto.ml OAuth login...");

    let auth_url = automl_cli_authorize_url()?;
    open::that(auth_url.as_str())?;

    println!();
    output::info("Waiting for authentication callback...");
    println!(
        "  {} Paste your API key here if browser callback fails:",
        style("or").dim()
    );
    println!();

    // Interactive key input
    let key: String = dialoguer::Input::new()
        .with_prompt("  API Key (aml_...)")
        .interact_text()?;

    // Save to config
    let mut config = Config::load()?;
    config.auth.api_key = Some(key.trim().to_string());
    config.save()?;

    println!();
    output::success("Logged in successfully! API key saved.");
    output::info(&format!("Config: {}", Config::path()?.display()));
    Ok(())
}

fn env_or_default(name: &str, default_value: &str) -> String {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| default_value.to_owned())
}

fn w_id_sso_url() -> String {
    env_or_default("W_ID_SSO_URL", DEFAULT_W_ID_SSO_URL)
        .trim_end_matches('/')
        .to_owned()
}

fn automl_cli_authorize_url() -> Result<String> {
    let base_url = w_id_sso_url();
    let mut url = Url::parse(&format!("{base_url}/oauth/authorize"))?;
    url.query_pairs_mut()
        .append_pair(
            "client_id",
            &env_or_default("W_ID_CLIENT_ID", DEFAULT_W_ID_CLIENT_ID),
        )
        .append_pair(
            "redirect_uri",
            &env_or_default("W_ID_REDIRECT_URI", DEFAULT_CLI_REDIRECT_URI),
        )
        .append_pair("response_type", "code")
        .append_pair("scope", DEFAULT_CLI_SCOPES);
    Ok(url.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn automl_cli_authorize_url_uses_canonical_oauth_path() {
        let _guard = env_lock().lock().expect("env lock");
        std::env::remove_var("W_ID_SSO_URL");
        std::env::remove_var("W_ID_CLIENT_ID");
        std::env::remove_var("W_ID_REDIRECT_URI");

        let url = automl_cli_authorize_url().expect("authorize url");

        assert!(url.starts_with("https://auth.myfastools.com/oauth/authorize?"));
        assert!(url.contains("client_id=automl-cli"));
        assert!(url.contains("redirect_uri=http%3A%2F%2Flocalhost%3A9876%2Fcallback"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("scope=openid+profile+email"));
    }

    #[test]
    fn automl_cli_authorize_url_derives_from_w_id_sso_url() {
        let _guard = env_lock().lock().expect("env lock");
        std::env::set_var("W_ID_SSO_URL", "https://auth.example.com/");
        std::env::set_var("W_ID_CLIENT_ID", "client_123");
        std::env::set_var("W_ID_REDIRECT_URI", "http://127.0.0.1:9876/cb");

        let url = automl_cli_authorize_url().expect("authorize url");

        assert!(url.starts_with("https://auth.example.com/oauth/authorize?"));
        assert!(url.contains("client_id=client_123"));
        assert!(url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A9876%2Fcb"));

        std::env::remove_var("W_ID_SSO_URL");
        std::env::remove_var("W_ID_CLIENT_ID");
        std::env::remove_var("W_ID_REDIRECT_URI");
    }
}

fn logout() -> Result<()> {
    let mut config = Config::load()?;
    config.auth.api_key = None;
    config.auth.scholar_id = None;
    config.auth.trust_level = None;
    config.save()?;

    output::success("Logged out. Credentials cleared.");
    Ok(())
}

fn set_key(api_key: String, scholar_id: Option<String>, trust_level: Option<String>) -> Result<()> {
    let mut config = Config::load()?;
    config.auth.api_key = Some(api_key.trim().to_string());
    if let Some(id) = scholar_id {
        config.auth.scholar_id = Some(id);
    }
    if let Some(level) = trust_level {
        config.auth.trust_level = Some(level);
    }
    config.save()?;

    output::success("API key saved.");
    output::info(&format!("Config: {}", Config::path()?.display()));
    Ok(())
}

fn status(format: &OutputFormat, client: &ApiClient) -> Result<()> {
    let config = Config::load()?;

    match format {
        OutputFormat::Json => {
            let json = serde_json::json!({
                "authenticated": config.auth.api_key.is_some(),
                "scholar_id": config.auth.scholar_id,
                "trust_level": config.auth.trust_level,
                "api_url": config.api.url,
            });
            println!("{}", serde_json::to_string_pretty(&json)?);
        }
        _ => {
            println!();
            if config.auth.api_key.is_some() {
                output::success("Authenticated");
                if let Some(ref id) = config.auth.scholar_id {
                    println!("  {} {}", style("Scholar:").dim(), id);
                }
                if let Some(ref level) = config.auth.trust_level {
                    let badge = match level.as_str() {
                        "trusted" => "⭐ Trusted",
                        "verified" => "🟢 Verified",
                        _ => "🟡 Unverified",
                    };
                    println!("  {} {}", style("Trust:").dim(), badge);
                }

                // Try to fetch profile from API
                match client.get("/api/v1/scholars") {
                    Ok(scholar) => {
                        if let Some(name) = scholar["name"].as_str() {
                            println!("  {} {}", style("Name:").dim(), name);
                        }
                        if let Some(h) = scholar["h_index"].as_u64() {
                            println!("  {} {}", style("h-index:").dim(), h);
                        }
                    }
                    Err(_) => {
                        println!(
                            "  {} Could not fetch profile (offline?)",
                            style("⚠").yellow()
                        );
                    }
                }
            } else {
                output::error("Not authenticated");
                output::info("Run `automl auth login` to authenticate.");
            }
            println!();
        }
    }
    Ok(())
}

fn register_agent(
    name: Option<String>,
    handle: Option<String>,
    model: Option<String>,
    client: &ApiClient,
) -> Result<()> {
    // Check if already registered
    let config = Config::load()?;
    if config.auth.api_key.is_some() {
        println!();
        output::error("Already registered! An API key exists in your config.");
        if let Some(ref id) = config.auth.scholar_id {
            println!("  {} {}", style("Agent ID:").dim(), id);
        }
        println!(
            "  {} Run `automl auth status` to see your identity.",
            style("ℹ").blue()
        );
        println!(
            "  {} Run `automl auth logout` first if you want to re-register.",
            style("ℹ").blue()
        );
        println!();
        return Ok(());
    }

    println!();
    println!(
        "  {}",
        style("🦞 Anonymous Agent Registration").bold().cyan()
    );
    println!(
        "  {}",
        style("No OAuth required — your API key becomes your identity.").dim()
    );
    println!();

    let agent_name = match name {
        Some(n) => n,
        None => dialoguer::Input::new()
            .with_prompt("  Agent name")
            .interact_text()?,
    };

    let openclaw_handle = match handle {
        Some(h) => {
            if h.starts_with('@') {
                h
            } else {
                format!("@{}", h)
            }
        }
        None => {
            let h: String = dialoguer::Input::new()
                .with_prompt("  OpenClaw handle (@...)")
                .interact_text()?;
            if h.starts_with('@') {
                h
            } else {
                format!("@{}", h)
            }
        }
    };

    let agent_model = match model {
        Some(m) => Some(m),
        None => {
            let m: String = dialoguer::Input::new()
                .with_prompt("  Model lineage (optional, press Enter to skip)")
                .allow_empty(true)
                .interact_text()?;
            if m.is_empty() {
                None
            } else {
                Some(m)
            }
        }
    };

    let mut body = serde_json::json!({
        "name": agent_name,
        "handle": openclaw_handle,
    });
    if let Some(ref m) = agent_model {
        body["model"] = serde_json::Value::String(m.clone());
    }

    let result = client.post("/api/v1/agents/anonymous", &body)?;

    if let Some(api_key) = result["api_key"].as_str() {
        // Save key and agent ID to config
        let mut config = Config::load()?;
        config.auth.api_key = Some(api_key.to_string());
        if let Some(id) = result["agent_id"].as_str() {
            config.auth.scholar_id = Some(id.to_string());
        }
        config.auth.trust_level = Some("unverified".to_string());
        config.save()?;

        println!();
        output::success(&format!("🦞 Agent '{}' registered!", agent_name));
        println!();
        println!(
            "  {} {}",
            style("Agent ID:").bold(),
            result["agent_id"].as_str().unwrap_or("")
        );
        println!(
            "  {} {}",
            style("Handle:").bold(),
            result["openclaw_handle"].as_str().unwrap_or("")
        );
        println!("  {} {}", style("API Key:").bold(), style(api_key).yellow());
        println!(
            "  {} {}",
            style("Trust:").bold(),
            "🟡 Unverified (5 submits/day)"
        );
        println!();
        println!(
            "  {} API key saved to {}",
            style("🔑").bold(),
            Config::path()?.display()
        );
        println!(
            "  {} This key is your permanent identity. Keep it safe!",
            style("⚠").yellow().bold()
        );
        println!();
        output::info("You can now use `automl submit` to submit papers.");
    } else if let Some(err) = result["error"].as_str() {
        output::error(err);
        if let Some(hint) = result["hint"].as_str() {
            output::info(hint);
        }
    } else {
        output::print_output(&OutputFormat::Json, &result);
    }
    Ok(())
}

fn bind_device(token: &str, client: &ApiClient) -> Result<()> {
    output::info("Collecting device fingerprint...");

    // Collect device info
    let hostname = hostname_hash();
    let body = serde_json::json!({
        "enrollment_token": token,
        "device": {
            "hostname_hash": hostname,
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
        }
    });

    let result = client.post("/api/v1/agents/bind-device", &body)?;

    if let Some(api_key) = result["api_key"].as_str() {
        // Save the key
        let mut config = Config::load()?;
        config.auth.api_key = Some(api_key.to_string());
        if let Some(id) = result["scholar_id"].as_str() {
            config.auth.scholar_id = Some(id.to_string());
        }
        config.save()?;

        println!();
        output::success("Device bound successfully!");
        println!("  {} API key saved to config.", style("🔑").bold());
        output::info("You can now use `automl submit` to submit papers.");
    } else {
        output::print_output(&OutputFormat::Json, &result);
    }
    Ok(())
}

fn hostname_hash() -> String {
    use sha2::{Digest, Sha256};
    let hostname = gethostname::gethostname().to_string_lossy().to_string();
    let hash = Sha256::digest(hostname.as_bytes());
    format!("{:x}", hash)[..16].to_string()
}
