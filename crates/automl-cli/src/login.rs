//! `automl login` — loopback OAuth-style flow with PKCE against the platform.
//!
//! 1. generate verifier, start a 127.0.0.1 listener on a random port
//! 2. open the browser at `/api/v1/cli/auth/start?redirect_uri=…&challenge=…`
//!    (the server bounces through W-ID SSO when there is no session)
//! 3. receive the one-time code on the loopback callback
//! 4. exchange code+verifier at `/api/v1/cli/auth/token` for an `amk_*` key
//! 5. persist to `~/.config/automl/credentials.toml` (0600)
//!
//! `--with-api-key` is the no-browser fallback: the key is read from stdin so
//! it never lands in shell history.

use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    process::Command,
};

use rand::RngCore;
use serde_json::{json, Value};

use crate::{
    client::{urlencode, Api},
    creds::{self, Credentials},
};

pub fn login(api: &Api, with_api_key: bool) -> anyhow::Result<Value> {
    if with_api_key {
        return login_with_api_key(api);
    }

    let mut raw = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut raw);
    let verifier = w_crypto::hex_encode(&raw);
    let challenge = w_crypto::sha256_hex(verifier.as_bytes());

    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    let redirect_uri = format!("http://127.0.0.1:{port}/callback");
    let start_url = format!(
        "{}/api/v1/cli/auth/start?redirect_uri={}&challenge={}",
        api.base,
        urlencode(&redirect_uri),
        urlencode(&challenge)
    );

    eprintln!("Opening browser for W-ID login…");
    eprintln!("If it does not open, visit:\n\n  {start_url}\n");
    open_browser(&start_url);
    eprintln!("Waiting for the browser to complete login (Ctrl+C to abort)…");

    let code = wait_for_code(&listener)?;

    let host = std::env::var("HOSTNAME").unwrap_or_else(|_| "host".into());
    let created = api.post(
        "/api/v1/cli/auth/token",
        &json!({
            "code": code,
            "verifier": verifier,
            "name": format!("cli-{host}"),
        }),
    )?;

    persist(api, &created)?;
    Ok(summary(&created))
}

fn login_with_api_key(api: &Api) -> anyhow::Result<Value> {
    eprintln!("Paste your API key (amk_…), then press Enter:");
    let mut key = String::new();
    std::io::stdin().read_line(&mut key)?;
    let key = key.trim().to_string();
    if !key.starts_with("amk_") {
        anyhow::bail!("that does not look like an automl API key (expected amk_…)");
    }
    // Validate before persisting — a typo'd key should fail loudly now.
    let probe = Api {
        base: api.base.clone(),
        http: api.http.clone(),
        api_key: Some(key.clone()),
    };
    let keys = probe.get("/api/v1/keys?limit=1")?;
    let key_id = keys["data"][0]["id"].as_str().unwrap_or("").to_string();
    creds::save_credentials(&Credentials {
        server: api.base.clone(),
        api_key: key,
        key_id,
    })?;
    Ok(json!({
        "status": "logged_in",
        "credentials": creds::credentials_path().to_string_lossy(),
    }))
}

fn persist(api: &Api, created: &Value) -> anyhow::Result<()> {
    let key = created["key"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("token exchange returned no key"))?;
    creds::save_credentials(&Credentials {
        server: api.base.clone(),
        api_key: key.to_string(),
        key_id: created["id"].as_str().unwrap_or("").to_string(),
    })
}

fn summary(created: &Value) -> Value {
    json!({
        "status": "logged_in",
        "key_id": created["id"],
        "key_name": created["name"],
        "scopes": created["scopes"],
        "credentials": creds::credentials_path().to_string_lossy(),
    })
}

/// Accept loopback connections until the `/callback?code=…` request arrives.
fn wait_for_code(listener: &TcpListener) -> anyhow::Result<String> {
    for stream in listener.incoming() {
        let mut stream = stream?;
        stream.set_read_timeout(Some(std::time::Duration::from_secs(10)))?;
        let mut reader = BufReader::new(stream.try_clone()?);
        let mut request_line = String::new();
        if reader.read_line(&mut request_line).is_err() {
            continue;
        }
        // Drain the rest of the headers so browsers don't see a reset.
        let mut line = String::new();
        while reader.read_line(&mut line).is_ok() && line.trim() != "" {
            line.clear();
        }

        let path = request_line.split_whitespace().nth(1).unwrap_or("");
        if let Some(code) = extract_code(path) {
            respond(
                &mut stream,
                200,
                "<h2>Auto.ml CLI login complete.</h2><p>You can close this tab and return to the terminal.</p>",
            );
            return Ok(code);
        }
        respond(&mut stream, 404, "<p>Not found.</p>");
    }
    anyhow::bail!("loopback listener closed before a login callback arrived")
}

pub fn extract_code(path: &str) -> Option<String> {
    let (route, query) = path.split_once('?')?;
    if route != "/callback" {
        return None;
    }
    for pair in query.split('&') {
        if let Some(code) = pair.strip_prefix("code=") {
            if !code.is_empty() && code.chars().all(|c| c.is_ascii_hexdigit()) {
                return Some(code.to_string());
            }
        }
    }
    None
}

fn respond(stream: &mut std::net::TcpStream, status: u16, body_html: &str) {
    let body = format!(
        "<!doctype html><html><body style=\"font-family:Georgia,serif;margin:3rem\">{body_html}</body></html>"
    );
    let reason = if status == 200 { "OK" } else { "Not Found" };
    let _ = stream.write_all(
        format!(
            "HTTP/1.1 {status} {reason}\r\ncontent-type: text/html; charset=utf-8\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        )
        .as_bytes(),
    );
    let _ = stream.flush();
    // Give the peer a moment to read before we drop the socket.
    let mut drain = [0u8; 64];
    let _ = stream.read(&mut drain);
}

fn open_browser(url: &str) {
    #[cfg(target_os = "macos")]
    let cmds: &[&str] = &["open"];
    #[cfg(not(target_os = "macos"))]
    let cmds: &[&str] = &["xdg-open", "sensible-browser"];
    for cmd in cmds {
        if Command::new(cmd).arg(url).spawn().is_ok() {
            return;
        }
    }
}

pub fn logout(api: &Api) -> anyhow::Result<Value> {
    let Some(saved) = creds::load_credentials() else {
        return Ok(json!({"status": "not_logged_in"}));
    };
    // Best-effort self-revocation of the stored key.
    let mut revoked = false;
    if !saved.key_id.is_empty() {
        let authed = Api {
            base: saved.server.clone(),
            http: api.http.clone(),
            api_key: Some(saved.api_key.clone()),
        };
        revoked = authed
            .delete(&format!("/api/v1/keys/{}", saved.key_id))
            .is_ok();
    }
    creds::delete_credentials();
    Ok(json!({"status": "logged_out", "key_revoked": revoked}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_code_only_from_callback() {
        assert_eq!(
            extract_code("/callback?code=deadbeef").as_deref(),
            Some("deadbeef")
        );
        assert_eq!(
            extract_code("/callback?state=x&code=00ff").as_deref(),
            Some("00ff")
        );
        assert!(extract_code("/favicon.ico").is_none());
        assert!(extract_code("/callback").is_none());
        assert!(extract_code("/callback?code=").is_none());
        assert!(extract_code("/evil?code=deadbeef").is_none());
        // Non-hex injection is rejected.
        assert!(extract_code("/callback?code=<script>").is_none());
    }
}
