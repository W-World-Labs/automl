//! HTTP layer: authed requests, error-envelope decoding, cursor pagination.

use serde_json::Value;

/// Thin wrapper over the blocking client carrying base URL + credential.
pub struct Api {
    pub base: String,
    pub http: reqwest::blocking::Client,
    pub api_key: Option<String>,
}

pub const LOGIN_HINT: &str =
    "unauthorized — run `automl login` first (or pass --api-key / set AUTOML_API_KEY)";

impl Api {
    pub fn require_key(&self) -> anyhow::Result<&str> {
        self.api_key.as_deref().ok_or_else(|| {
            anyhow::anyhow!(
                "no credential found — run `automl login` (or pass --api-key / set AUTOML_API_KEY)"
            )
        })
    }

    pub fn get(&self, path: &str) -> anyhow::Result<Value> {
        self.send(reqwest::Method::GET, path, None)
    }

    pub fn post(&self, path: &str, body: &Value) -> anyhow::Result<Value> {
        self.send(reqwest::Method::POST, path, Some(body))
    }

    pub fn patch(&self, path: &str, body: &Value) -> anyhow::Result<Value> {
        self.send(reqwest::Method::PATCH, path, Some(body))
    }

    pub fn delete(&self, path: &str) -> anyhow::Result<Value> {
        self.send(reqwest::Method::DELETE, path, None)
    }

    /// One authed request. Non-2xx decodes the `{"error":{code,message}}`
    /// envelope; 401 always carries the login hint instead of a bare error.
    pub fn send(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&Value>,
    ) -> anyhow::Result<Value> {
        let mut req = self.http.request(method, format!("{}{path}", self.base));
        if let Some(key) = self.api_key.as_deref() {
            req = req.header("authorization", format!("Bearer {key}"));
        }
        if let Some(b) = body {
            req = req.json(b);
        }
        let resp = req.send()?;
        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED {
            anyhow::bail!("{LOGIN_HINT}");
        }
        let text = resp.text().unwrap_or_default();
        if !status.is_success() {
            let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
            let code = v["error"]["code"].as_str().unwrap_or("HTTP_ERROR");
            let message = v["error"]["message"].as_str().unwrap_or(text.trim());
            anyhow::bail!("{code}: {message} (HTTP {})", status.as_u16());
        }
        if text.trim().is_empty() {
            return Ok(Value::Null);
        }
        Ok(serde_json::from_str(&text)?)
    }

    /// Follow `next_cursor` until `limit` items are collected (or exhaustion
    /// with `all`). `path` must already contain its own query params or none.
    pub fn get_paged(&self, path: &str, limit: u32, all: bool) -> anyhow::Result<Vec<Value>> {
        let mut items = Vec::new();
        let mut cursor: Option<String> = None;
        let sep = if path.contains('?') { '&' } else { '?' };
        loop {
            let remaining = if all {
                200
            } else {
                (limit as usize).saturating_sub(items.len()).min(200)
            };
            if remaining == 0 {
                break;
            }
            let mut url = format!("{path}{sep}limit={remaining}");
            if let Some(c) = &cursor {
                url.push_str(&format!("&cursor={}", urlencode(c)));
            }
            let page = self.get(&url)?;
            if let Some(data) = page["data"].as_array() {
                items.extend(data.iter().cloned());
            }
            match page["next_cursor"].as_str() {
                Some(c) if all || items.len() < limit as usize => cursor = Some(c.to_string()),
                _ => break,
            }
        }
        Ok(items)
    }

    /// Retry transient failures (transport error / 5xx / 429) with backoff —
    /// used for evidence upload so a flaky link doesn't lose a finished run.
    pub fn put_with_retry(&self, path: &str, body: &Value) -> anyhow::Result<Value> {
        let mut delay = std::time::Duration::from_secs(1);
        let mut last_err: Option<anyhow::Error> = None;
        for attempt in 0..4 {
            if attempt > 0 {
                std::thread::sleep(delay);
                delay *= 2;
            }
            match self.try_put_raw(path, body) {
                Ok(Retryable::Done(v)) => return Ok(v),
                Ok(Retryable::Retry(msg)) => last_err = Some(anyhow::anyhow!(msg)),
                Err(e) => return Err(e),
            }
        }
        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("evidence upload failed after retries")))
    }

    fn try_put_raw(&self, path: &str, body: &Value) -> anyhow::Result<Retryable> {
        let mut req = self
            .http
            .request(reqwest::Method::PUT, format!("{}{path}", self.base))
            .json(body);
        if let Some(key) = self.api_key.as_deref() {
            req = req.header("authorization", format!("Bearer {key}"));
        }
        let resp = match req.send() {
            Ok(r) => r,
            Err(e) if e.is_timeout() || e.is_connect() || e.is_request() => {
                return Ok(Retryable::Retry(format!("transport error: {e}")));
            }
            Err(e) => return Err(e.into()),
        };
        let status = resp.status();
        if status.is_server_error() || status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Ok(Retryable::Retry(format!("server answered {status}")));
        }
        if status == reqwest::StatusCode::UNAUTHORIZED {
            anyhow::bail!("{LOGIN_HINT}");
        }
        let text = resp.text().unwrap_or_default();
        if !status.is_success() {
            let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
            anyhow::bail!(
                "{}: {} (HTTP {})",
                v["error"]["code"].as_str().unwrap_or("HTTP_ERROR"),
                v["error"]["message"].as_str().unwrap_or(text.trim()),
                status.as_u16()
            );
        }
        Ok(Retryable::Done(serde_json::from_str(&text)?))
    }
}

enum Retryable {
    Done(Value),
    Retry(String),
}

pub fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
