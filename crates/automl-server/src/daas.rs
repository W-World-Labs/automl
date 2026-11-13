//! W-DaaS facade for Research Hub P2 (docs/RESEARCH-HUB-DESIGN.md §12.4/§12.7).
//!
//! The default `StubDaaS` keeps dev and CI free of the external data platform;
//! `DAAS_API_BASE` selects the bounded HTTP adapter. The live adapter assumes
//! `GET {base}/api/workspaces/{id}`, `POST {base}/api/workspaces/{id}/documents`
//! and `GET {base}/api/workspaces/{id}/search?q=&limit=` — a best-effort
//! contract pending the cross-repository W-DaaS follow-up issue.

use std::{future::Future, pin::Pin, time::Duration};

use serde::{Deserialize, Serialize};

use crate::{config::DaasConfig, error::ApiError};

const RESPONSE_LIMIT: usize = 512 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(4);

/// A workspace lookup. `verified` reports whether the provider confirmed the
/// workspace exists; the stub can never verify, and upstream outages degrade
/// to `verified: false` instead of failing the bind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaasWorkspace {
    pub workspace_id: String,
    pub display_name: Option<String>,
    pub verified: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaasDocument {
    pub document_id: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct DaasHit {
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub score: Option<f64>,
    #[serde(default)]
    pub uri: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
}

/// Upstream call failed; the API layer maps this to 503.
#[derive(Debug)]
pub struct DaasFailure(pub String);

impl From<DaasFailure> for ApiError {
    fn from(e: DaasFailure) -> Self {
        ApiError::unavailable(format!("W-DaaS upstream unavailable: {}", e.0))
    }
}

trait Provider: Send + Sync {
    fn resolve_workspace<'a>(
        &'a self,
        workspace_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = DaasWorkspace> + Send + 'a>>;

    fn import_document<'a>(
        &'a self,
        workspace_id: &'a str,
        title: &'a str,
        source_uri: Option<&'a str>,
    ) -> Pin<Box<dyn Future<Output = Result<DaasDocument, DaasFailure>> + Send + 'a>>;

    fn hybrid_search<'a>(
        &'a self,
        workspace_id: &'a str,
        query: &'a str,
        limit: u32,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<DaasHit>, DaasFailure>> + Send + 'a>>;
}

/// Selected provider plus the shared bounded HTTP client.
pub struct Daas {
    provider: ProviderKind,
}

enum ProviderKind {
    Stub(StubDaaS),
    Http(HttpDaaS),
}

impl Daas {
    pub fn new(config: &DaasConfig) -> Self {
        crate::ensure_rustls_crypto_provider();
        let provider = match config.api_base.as_deref() {
            Some(base) => ProviderKind::Http(HttpDaaS::new(base, config.api_token.clone())),
            None => ProviderKind::Stub(StubDaaS),
        };
        Self { provider }
    }

    pub fn stub() -> Self {
        Self::new(&DaasConfig::default())
    }

    pub fn provider_name(&self) -> &'static str {
        match &self.provider {
            ProviderKind::Stub(_) => "stub",
            ProviderKind::Http(_) => "daas",
        }
    }

    /// Workspace lookup never fails: upstream outages and stub mode both
    /// yield `verified: false` so a bind records an honest unverified pointer.
    pub async fn resolve_workspace(&self, workspace_id: &str) -> DaasWorkspace {
        match &self.provider {
            ProviderKind::Stub(p) => p.resolve_workspace(workspace_id).await,
            ProviderKind::Http(p) => p.resolve_workspace(workspace_id).await,
        }
    }

    pub async fn import_document(
        &self,
        workspace_id: &str,
        title: &str,
        source_uri: Option<&str>,
    ) -> Result<DaasDocument, DaasFailure> {
        match &self.provider {
            ProviderKind::Stub(p) => p.import_document(workspace_id, title, source_uri).await,
            ProviderKind::Http(p) => p.import_document(workspace_id, title, source_uri).await,
        }
    }

    pub async fn hybrid_search(
        &self,
        workspace_id: &str,
        query: &str,
        limit: u32,
    ) -> Result<Vec<DaasHit>, DaasFailure> {
        match &self.provider {
            ProviderKind::Stub(p) => p.hybrid_search(workspace_id, query, limit).await,
            ProviderKind::Http(p) => p.hybrid_search(workspace_id, query, limit).await,
        }
    }
}

/// Deterministic provider for dev/CI: no corpus, no verification, synthetic
/// `stub-doc-*` document ids so pointer columns stay honest about provenance.
pub struct StubDaaS;

impl Provider for StubDaaS {
    fn resolve_workspace<'a>(
        &'a self,
        workspace_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = DaasWorkspace> + Send + 'a>> {
        Box::pin(async move {
            DaasWorkspace {
                workspace_id: workspace_id.to_string(),
                display_name: None,
                verified: false,
            }
        })
    }

    fn import_document<'a>(
        &'a self,
        _workspace_id: &'a str,
        _title: &'a str,
        _source_uri: Option<&'a str>,
    ) -> Pin<Box<dyn Future<Output = Result<DaasDocument, DaasFailure>> + Send + 'a>> {
        Box::pin(async move {
            Ok(DaasDocument {
                document_id: format!(
                    "stub-doc-{}",
                    &uuid::Uuid::new_v4().simple().to_string()[..12]
                ),
            })
        })
    }

    fn hybrid_search<'a>(
        &'a self,
        _workspace_id: &'a str,
        _query: &'a str,
        _limit: u32,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<DaasHit>, DaasFailure>> + Send + 'a>> {
        // The stub has no corpus: an empty page is the honest answer.
        Box::pin(async move { Ok(Vec::new()) })
    }
}

struct HttpDaaS {
    base: String,
    token: Option<String>,
    http: reqwest::Client,
}

impl HttpDaaS {
    fn new(base: &str, token: Option<String>) -> Self {
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("valid DaaS HTTP client configuration");
        Self {
            base: base.trim_end_matches('/').to_owned(),
            token,
            http,
        }
    }

    fn get(&self, url: &str) -> reqwest::RequestBuilder {
        let req = self.http.get(url);
        match &self.token {
            Some(token) => req.bearer_auth(token),
            None => req,
        }
    }
}

impl Provider for HttpDaaS {
    fn resolve_workspace<'a>(
        &'a self,
        workspace_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = DaasWorkspace> + Send + 'a>> {
        Box::pin(async move {
            let unverified = || DaasWorkspace {
                workspace_id: workspace_id.to_string(),
                display_name: None,
                verified: false,
            };
            let url = format!(
                "{}/api/workspaces/{}",
                self.base,
                path_segment(workspace_id)
            );
            let Ok(response) = self.get(&url).send().await else {
                return unverified();
            };
            if !response.status().is_success() {
                return unverified();
            }
            let Some(body) = read_body(response).await else {
                return unverified();
            };
            #[derive(Deserialize)]
            struct WorkspaceRow {
                id: Option<String>,
                workspace_id: Option<String>,
                name: Option<String>,
                display_name: Option<String>,
            }
            let row: WorkspaceRow = serde_json::from_slice(&body).unwrap_or(WorkspaceRow {
                id: None,
                workspace_id: None,
                name: None,
                display_name: None,
            });
            DaasWorkspace {
                workspace_id: row
                    .id
                    .or(row.workspace_id)
                    .unwrap_or_else(|| workspace_id.to_string()),
                display_name: row.name.or(row.display_name),
                verified: true,
            }
        })
    }

    fn import_document<'a>(
        &'a self,
        workspace_id: &'a str,
        title: &'a str,
        source_uri: Option<&'a str>,
    ) -> Pin<Box<dyn Future<Output = Result<DaasDocument, DaasFailure>> + Send + 'a>> {
        Box::pin(async move {
            let url = format!(
                "{}/api/workspaces/{}/documents",
                self.base,
                path_segment(workspace_id)
            );
            let mut req = self.http.post(&url).json(&serde_json::json!({
                "title": title,
                "source_uri": source_uri,
            }));
            if let Some(token) = &self.token {
                req = req.bearer_auth(token);
            }
            let response = req
                .send()
                .await
                .map_err(|e| DaasFailure(format!("document import failed: {e}")))?;
            if !response.status().is_success() {
                return Err(DaasFailure(format!(
                    "document import returned HTTP {}",
                    response.status()
                )));
            }
            let body = read_body(response)
                .await
                .ok_or_else(|| DaasFailure("document import returned an oversized body".into()))?;
            #[derive(Deserialize)]
            struct DocumentRow {
                id: Option<String>,
                document_id: Option<String>,
            }
            let row: DocumentRow = serde_json::from_slice(&body)
                .map_err(|_| DaasFailure("document import returned malformed JSON".into()))?;
            let document_id = row
                .id
                .or(row.document_id)
                .ok_or_else(|| DaasFailure("document import returned no id".into()))?;
            Ok(DaasDocument { document_id })
        })
    }

    fn hybrid_search<'a>(
        &'a self,
        workspace_id: &'a str,
        query: &'a str,
        limit: u32,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<DaasHit>, DaasFailure>> + Send + 'a>> {
        Box::pin(async move {
            let url = format!(
                "{}/api/workspaces/{}/search?q={}&limit={limit}",
                self.base,
                path_segment(workspace_id),
                query_segment(query),
            );
            let response = self
                .get(&url)
                .send()
                .await
                .map_err(|e| DaasFailure(format!("hybrid search failed: {e}")))?;
            if !response.status().is_success() {
                return Err(DaasFailure(format!(
                    "hybrid search returned HTTP {}",
                    response.status()
                )));
            }
            let body = read_body(response)
                .await
                .ok_or_else(|| DaasFailure("hybrid search returned an oversized body".into()))?;
            // Tolerate both `{hits: [...]}` and a bare array payload.
            #[derive(Deserialize)]
            struct HitsEnvelope {
                hits: Option<Vec<DaasHit>>,
            }
            if let Ok(envelope) = serde_json::from_slice::<HitsEnvelope>(&body) {
                if let Some(hits) = envelope.hits {
                    return Ok(hits.into_iter().take(limit as usize).collect());
                }
            }
            let hits: Vec<DaasHit> = serde_json::from_slice(&body)
                .map_err(|_| DaasFailure("hybrid search returned malformed JSON".into()))?;
            Ok(hits.into_iter().take(limit as usize).collect())
        })
    }
}

async fn read_body(response: reqwest::Response) -> Option<Vec<u8>> {
    if response
        .content_length()
        .is_some_and(|n| n > RESPONSE_LIMIT as u64)
    {
        return None;
    }
    let mut response = response;
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        if body.len() + chunk.len() > RESPONSE_LIMIT {
            return None;
        }
        body.extend_from_slice(&chunk);
    }
    Some(body)
}

/// Encode an untrusted identifier as exactly one URL path segment.
fn path_segment(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            result.push(byte as char);
        } else {
            result.push_str(&format!("%{byte:02X}"));
        }
    }
    result
}

/// Percent-encode a query-string value.
fn query_segment(value: &str) -> String {
    path_segment(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stub_never_verifies_and_mints_stub_document_ids() {
        let daas = Daas::stub();
        assert_eq!(daas.provider_name(), "stub");
        let ws = daas.resolve_workspace("ws-1").await;
        assert_eq!(ws.workspace_id, "ws-1");
        assert!(!ws.verified);
        let doc = daas
            .import_document("ws-1", "paper.pdf", None)
            .await
            .unwrap();
        assert!(doc.document_id.starts_with("stub-doc-"));
        assert!(daas
            .hybrid_search("ws-1", "attention", 10)
            .await
            .unwrap()
            .is_empty());
    }

    #[test]
    fn untrusted_ids_cannot_escape_a_path_segment() {
        assert_eq!(path_segment("../a/b ?"), "..%2Fa%2Fb%20%3F");
        assert_eq!(query_segment("a&b=c"), "a%26b%3Dc");
    }
}
