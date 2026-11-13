// ============================================================
// Auto.ml CLI — API Client (reqwest blocking)
// ============================================================

use anyhow::{Context, Result};
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use serde::Serialize;

use crate::config::Config;

pub struct ApiClient {
    client: Client,
    base_url: String,
    api_key: Option<String>,
}

impl ApiClient {
    /// Create a new API client from config + optional overrides
    pub fn new(api_url_override: Option<&str>, api_key_override: Option<&str>) -> Result<Self> {
        let config = Config::load()?;
        let base_url = api_url_override
            .map(|s| s.to_string())
            .unwrap_or(config.api.url)
            .trim_end_matches('/')
            .to_string();

        let api_key = api_key_override
            .map(|s| s.to_string())
            .or(config.auth.api_key);

        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()?;

        Ok(Self {
            client,
            base_url,
            api_key,
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    fn headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(USER_AGENT, HeaderValue::from_static("automl-cli/0.3.0"));
        if let Some(ref key) = self.api_key {
            if let Ok(val) = HeaderValue::from_str(&format!("Bearer {key}")) {
                headers.insert(AUTHORIZATION, val);
            }
        }
        headers
    }

    /// GET request
    pub fn get(&self, path: &str) -> Result<serde_json::Value> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self
            .client
            .get(&url)
            .headers(self.headers())
            .send()
            .with_context(|| format!("GET {url} failed"))?;

        let status = resp.status();
        let body: serde_json::Value = resp
            .json()
            .with_context(|| format!("Failed to parse response from GET {url}"))?;

        if !status.is_success() {
            let error_msg = body["error"].as_str().unwrap_or("Unknown error");
            anyhow::bail!("API error (HTTP {status}): {error_msg}");
        }
        Ok(body)
    }

    /// GET with query parameters
    pub fn get_with_params(
        &self,
        path: &str,
        params: &[(&str, &str)],
    ) -> Result<serde_json::Value> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self
            .client
            .get(&url)
            .headers(self.headers())
            .query(params)
            .send()
            .with_context(|| format!("GET {url} failed"))?;

        let status = resp.status();
        let body: serde_json::Value = resp
            .json()
            .with_context(|| format!("Failed to parse response from GET {url}"))?;

        if !status.is_success() {
            let error_msg = body["error"].as_str().unwrap_or("Unknown error");
            anyhow::bail!("API error (HTTP {status}): {error_msg}");
        }
        Ok(body)
    }

    /// POST request with JSON body
    pub fn post<T: Serialize>(&self, path: &str, body: &T) -> Result<serde_json::Value> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self
            .client
            .post(&url)
            .headers(self.headers())
            .json(body)
            .send()
            .with_context(|| format!("POST {url} failed"))?;

        let status = resp.status();
        let raw_text = resp
            .text()
            .with_context(|| format!("Failed to read response from POST {url}"))?;

        let body: serde_json::Value = serde_json::from_str(&raw_text).with_context(|| {
            format!(
                "Failed to parse JSON from POST {url}: {}",
                &raw_text[..raw_text.len().min(200)]
            )
        })?;

        if !status.is_success() {
            let error_msg = body["error"].as_str().unwrap_or("Unknown error");
            let detail = body["detail"].as_str().unwrap_or("");
            if detail.is_empty() {
                anyhow::bail!("API error (HTTP {status}): {error_msg}");
            } else {
                anyhow::bail!("API error (HTTP {status}): {error_msg}\n  Detail: {detail}");
            }
        }
        Ok(body)
    }

    /// DELETE request
    pub fn delete(&self, path: &str) -> Result<serde_json::Value> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self
            .client
            .delete(&url)
            .headers(self.headers())
            .send()
            .with_context(|| format!("DELETE {url} failed"))?;

        let status = resp.status();
        let body: serde_json::Value = resp
            .json()
            .with_context(|| format!("Failed to parse response from DELETE {url}"))?;

        if !status.is_success() {
            let error_msg = body["error"].as_str().unwrap_or("Unknown error");
            anyhow::bail!("API error (HTTP {status}): {error_msg}");
        }
        Ok(body)
    }

    /// PATCH request with JSON body
    pub fn patch<T: Serialize>(&self, path: &str, body: &T) -> Result<serde_json::Value> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self
            .client
            .patch(&url)
            .headers(self.headers())
            .json(body)
            .send()
            .with_context(|| format!("PATCH {url} failed"))?;

        let status = resp.status();
        let raw_text = resp
            .text()
            .with_context(|| format!("Failed to read response from PATCH {url}"))?;

        let body: serde_json::Value = serde_json::from_str(&raw_text).with_context(|| {
            format!(
                "Failed to parse JSON from PATCH {url}: {}",
                &raw_text[..raw_text.len().min(200)]
            )
        })?;

        if !status.is_success() {
            let error_msg = body["error"].as_str().unwrap_or("Unknown error");
            anyhow::bail!("API error (HTTP {status}): {error_msg}");
        }
        Ok(body)
    }

    /// Upload file as multipart form to /api/assets
    pub fn upload_asset(
        &self,
        content: &[u8],
        filename: &str,
        content_type: &str,
        asset_type: &str,
    ) -> Result<serde_json::Value> {
        use reqwest::blocking::multipart;

        let url = format!("{}/api/assets", self.base_url);

        let file_part = multipart::Part::bytes(content.to_vec())
            .file_name(filename.to_string())
            .mime_str(content_type)
            .with_context(|| "Invalid content type")?;

        let form = multipart::Form::new()
            .part("file", file_part)
            .text("type", asset_type.to_string());

        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static("automl-cli/0.3.1"));
        if let Some(ref key) = self.api_key {
            if let Ok(val) = HeaderValue::from_str(&format!("Bearer {key}")) {
                headers.insert(AUTHORIZATION, val);
            }
        }

        let resp = self
            .client
            .post(&url)
            .headers(headers)
            .multipart(form)
            .send()
            .with_context(|| "Upload failed — check your API key and network")?;

        let status = resp.status();
        let raw_text = resp
            .text()
            .with_context(|| "Failed to read upload response")?;

        let body: serde_json::Value = serde_json::from_str(&raw_text).with_context(|| {
            format!("Invalid response: {}", &raw_text[..raw_text.len().min(200)])
        })?;

        if !status.is_success() {
            let error_msg = body["error"].as_str().unwrap_or("Unknown error");
            anyhow::bail!("Upload failed (HTTP {status}): {error_msg}");
        }
        Ok(body)
    }
}
