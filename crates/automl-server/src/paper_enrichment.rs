//! Best-effort metadata enrichment for Research Hub paper pointers.
//!
//! All provider failures intentionally collapse to empty metadata so external
//! availability never blocks adding a paper to a project.
//! The W Papers adapter assumes `GET {WPAPERS_BASE}/api/works/{work_id}` and
//! `GET {WPAPERS_BASE}/api/works/arxiv/{arxiv_id}` return OpenAlex-shaped JSON;
//! this is a best-effort cache contract, pending the cross-repository Issue #9.
use std::{future::Future, pin::Pin, time::Duration};

use serde::Deserialize;

use crate::config::EnrichmentConfig;

const RESPONSE_LIMIT: usize = 512 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaperIdentifier {
    Doi(String),
    Arxiv(String),
    WPapers(String),
    OpenAlex(String),
    DaasDocument(String),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PaperMetadata {
    pub title: Option<String>,
    pub year: Option<i32>,
    pub authors: Vec<String>,
    pub openalex_id: Option<String>,
    pub doi: Option<String>,
    pub w_papers_work_id: Option<String>,
}

pub trait Enricher: Send + Sync {
    fn enrich<'a>(
        &'a self,
        id: &'a PaperIdentifier,
    ) -> Pin<Box<dyn Future<Output = PaperMetadata> + Send + 'a>>;
}

/// Selected provider plus the optional W Papers title-cache source.
pub struct PaperEnrichment {
    provider: Provider,
    w_papers_base: Option<String>,
    http: reqwest::Client,
}

enum Provider {
    Stub(StubEnricher),
    Public(PublicOpenAlexEnricher),
    Daas(DaasOpenAlexEnricher),
}

impl PaperEnrichment {
    pub fn new(config: &EnrichmentConfig) -> Self {
        crate::ensure_rustls_crypto_provider();
        let provider = if let Some(base) = config.daas_openalex_base.as_deref() {
            Provider::Daas(DaasOpenAlexEnricher::new(base))
        } else if config.openalex_mode.as_deref() == Some("public") {
            Provider::Public(PublicOpenAlexEnricher)
        } else {
            Provider::Stub(StubEnricher)
        };
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(4))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("valid enrichment HTTP client configuration");
        Self {
            provider,
            w_papers_base: config.w_papers_base.clone(),
            http,
        }
    }

    pub fn stub() -> Self {
        Self::new(&EnrichmentConfig::default())
    }

    pub fn provider_name(&self) -> &'static str {
        match &self.provider {
            Provider::Stub(_) => "stub",
            Provider::Public(_) => "openalex-public",
            Provider::Daas(_) => "daas-openalex",
        }
    }

    pub async fn enrich(&self, id: &PaperIdentifier) -> PaperMetadata {
        // URL parsers normalize encoded `.` and `..` path components. Keep such
        // malformed pointers as raw ids and skip outbound lookup entirely.
        if identifier_value(id).is_some_and(|value| value == "." || value == "..") {
            return StubEnricher.enrich(id).await;
        }
        let mut metadata = match &self.provider {
            Provider::Stub(provider) => provider.enrich(id).await,
            Provider::Public(provider) => provider.enrich(id).await,
            Provider::Daas(provider) => provider.enrich(id).await,
        };
        if let PaperIdentifier::WPapers(work_id) = id {
            metadata.w_papers_work_id = Some(work_id.clone());
            if let Some(base) = &self.w_papers_base {
                let url = format!(
                    "{}/api/works/{}",
                    base.trim_end_matches('/'),
                    path_segment(work_id)
                );
                if let Some(found) = fetch_metadata(&self.http, &url, true).await {
                    merge_missing(&mut metadata, found);
                }
            }
        } else if let PaperIdentifier::Arxiv(arxiv_id) = id {
            if let Some(base) = &self.w_papers_base {
                let url = format!(
                    "{}/api/works/arxiv/{}",
                    base.trim_end_matches('/'),
                    path_segment(arxiv_id)
                );
                if let Some(found) = fetch_metadata(&self.http, &url, true).await {
                    merge_missing(&mut metadata, found);
                }
            }
        }
        metadata
    }
}

fn identifier_value(id: &PaperIdentifier) -> Option<&str> {
    match id {
        PaperIdentifier::Doi(value)
        | PaperIdentifier::Arxiv(value)
        | PaperIdentifier::WPapers(value)
        | PaperIdentifier::OpenAlex(value)
        | PaperIdentifier::DaasDocument(value) => Some(value),
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct StubEnricher;
impl Enricher for StubEnricher {
    fn enrich<'a>(
        &'a self,
        id: &'a PaperIdentifier,
    ) -> Pin<Box<dyn Future<Output = PaperMetadata> + Send + 'a>> {
        Box::pin(async move {
            let mut metadata = PaperMetadata::default();
            match id {
                PaperIdentifier::Doi(doi) => metadata.doi = Some(doi.clone()),
                PaperIdentifier::Arxiv(_) => {}
                PaperIdentifier::WPapers(work_id) => {
                    metadata.w_papers_work_id = Some(work_id.clone())
                }
                PaperIdentifier::OpenAlex(id) => metadata.openalex_id = Some(id.clone()),
                PaperIdentifier::DaasDocument(_) => {}
            }
            metadata
        })
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct PublicOpenAlexEnricher;
impl Enricher for PublicOpenAlexEnricher {
    fn enrich<'a>(
        &'a self,
        id: &'a PaperIdentifier,
    ) -> Pin<Box<dyn Future<Output = PaperMetadata> + Send + 'a>> {
        Box::pin(async move {
            if identifier_value(id).is_some_and(|value| value == "." || value == "..") {
                return StubEnricher.enrich(id).await;
            }
            let path = match id {
                PaperIdentifier::Doi(doi) => format!("works/https://doi.org/{}", path_segment(doi)),
                PaperIdentifier::OpenAlex(id) => format!("works/{}", path_segment(id)),
                _ => return StubEnricher.enrich(id).await,
            };
            fetch_metadata(
                &public_client(),
                &format!("https://api.openalex.org/{path}"),
                false,
            )
            .await
            .unwrap_or_default()
        })
    }
}

#[derive(Debug, Clone)]
pub struct DaasOpenAlexEnricher {
    base: String,
}
impl DaasOpenAlexEnricher {
    pub fn new(base: &str) -> Self {
        Self {
            base: base.trim_end_matches('/').to_owned(),
        }
    }
}
impl Enricher for DaasOpenAlexEnricher {
    fn enrich<'a>(
        &'a self,
        id: &'a PaperIdentifier,
    ) -> Pin<Box<dyn Future<Output = PaperMetadata> + Send + 'a>> {
        Box::pin(async move {
            if identifier_value(id).is_some_and(|value| value == "." || value == "..") {
                return StubEnricher.enrich(id).await;
            }
            let path = match id {
                PaperIdentifier::Doi(doi) => format!("works/https://doi.org/{}", path_segment(doi)),
                PaperIdentifier::OpenAlex(id) => format!("works/{}", path_segment(id)),
                _ => return StubEnricher.enrich(id).await,
            };
            fetch_metadata(&public_client(), &format!("{}/{path}", self.base), false)
                .await
                .unwrap_or_default()
        })
    }
}

fn public_client() -> reqwest::Client {
    crate::ensure_rustls_crypto_provider();
    reqwest::Client::builder()
        .timeout(Duration::from_secs(4))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("valid enrichment client")
}

#[derive(Deserialize)]
struct OpenAlexWork {
    display_name: Option<String>,
    title: Option<String>,
    publication_year: Option<i32>,
    year: Option<i32>,
    doi: Option<String>,
    id: Option<String>,
    authorships: Option<Vec<AuthorShip>>,
    authors: Option<Vec<String>>,
}
#[derive(Deserialize)]
struct AuthorShip {
    author: Option<Author>,
}
#[derive(Deserialize)]
struct Author {
    display_name: Option<String>,
}

async fn fetch_metadata(
    http: &reqwest::Client,
    url: &str,
    w_papers: bool,
) -> Option<PaperMetadata> {
    let response = http.get(url).send().await.ok()?;
    if !response.status().is_success()
        || response
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
    let work: OpenAlexWork = serde_json::from_slice(&body).ok()?;
    let year = work
        .publication_year
        .or(work.year)
        .filter(|year| (1000..=3000).contains(year));
    let mut authors = work
        .authorships
        .unwrap_or_default()
        .into_iter()
        .filter_map(|a| a.author.and_then(|v| v.display_name))
        .collect::<Vec<_>>();
    if authors.is_empty() {
        authors = work.authors.unwrap_or_default();
    }
    Some(PaperMetadata {
        title: clean(work.display_name.or(work.title)),
        year,
        authors: authors
            .into_iter()
            .filter_map(|author| clean(Some(author)))
            .collect(),
        openalex_id: if w_papers { None } else { clean(work.id) },
        doi: if w_papers { None } else { clean(work.doi) },
        w_papers_work_id: None,
    })
}

fn clean(value: Option<String>) -> Option<String> {
    let value = value?
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>();
    let value = value.trim().to_owned();
    (!value.is_empty()).then_some(value)
}

fn merge_missing(target: &mut PaperMetadata, found: PaperMetadata) {
    target.title = target.title.take().or(found.title);
    target.year = target.year.or(found.year);
    if target.authors.is_empty() {
        target.authors = found.authors;
    }
    target.openalex_id = target.openalex_id.take().or(found.openalex_id);
    target.doi = target.doi.take().or(found.doi);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stub_preserves_supported_identifiers_and_never_fetches_arxiv_org() {
        let enricher = PaperEnrichment::stub();
        assert_eq!(
            enricher
                .enrich(&PaperIdentifier::Doi("10.1000/x".into()))
                .await
                .doi
                .as_deref(),
            Some("10.1000/x")
        );
        assert!(enricher
            .enrich(&PaperIdentifier::Arxiv("2401.12345".into()))
            .await
            .title
            .is_none());
        assert_eq!(
            enricher
                .enrich(&PaperIdentifier::WPapers("wp-7".into()))
                .await
                .w_papers_work_id
                .as_deref(),
            Some("wp-7")
        );
    }

    #[test]
    fn untrusted_ids_cannot_escape_a_path_segment() {
        assert_eq!(path_segment("../a/b ?"), "..%2Fa%2Fb%20%3F");
    }

    #[tokio::test]
    async fn daas_provider_reads_bounded_openalex_metadata_from_configured_base() {
        use tokio::{
            io::{AsyncReadExt, AsyncWriteExt},
            net::TcpListener,
        };

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 2048];
            let size = socket.read(&mut request).await.unwrap();
            let request = String::from_utf8_lossy(&request[..size]);
            assert!(request.starts_with("GET /works/https://doi.org/10.1%2Fa%3Fb "));
            let body = r#"{"display_name":"A title","publication_year":2024,"doi":"https://doi.org/10.1/a?b","id":"https://openalex.org/W42","authorships":[{"author":{"display_name":"Ada"}}]}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(), body
            );
            socket.write_all(response.as_bytes()).await.unwrap();
        });
        let enricher = DaasOpenAlexEnricher::new(&format!("http://{address}"));
        let found = enricher
            .enrich(&PaperIdentifier::Doi("10.1/a?b".into()))
            .await;
        server.await.unwrap();
        assert_eq!(found.title.as_deref(), Some("A title"));
        assert_eq!(found.year, Some(2024));
        assert_eq!(found.authors, ["Ada"]);
        assert_eq!(
            found.openalex_id.as_deref(),
            Some("https://openalex.org/W42")
        );
    }
}
