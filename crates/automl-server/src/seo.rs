//! Search + answer-engine surface: robots.txt, sitemap.xml, llms.txt, and
//! server-side head injection for paper pages.
//!
//! AI crawlers and most social scrapers do NOT execute JavaScript, so paper
//! metadata (title/description/ScholarlyArticle JSON-LD) is injected into the
//! SPA shell server-side — client-side injection would be invisible to them.
//! Schema properties are populated only from real stored content; nothing is
//! invented (empty fields are omitted).

use axum::{
    extract::State,
    http::header,
    response::{IntoResponse, Response},
};
use uuid::Uuid;

use crate::{domain::papers::Paper, state::AppState};

pub async fn robots_txt(State(state): State<AppState>) -> Response {
    let base = &state.cfg.public_base_url;
    let body = format!(
        "User-agent: *\nAllow: /\nDisallow: /api/\nDisallow: /auth/\n\nSitemap: {base}/sitemap.xml\n"
    );
    ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], body).into_response()
}

/// Agent-readable site map (GEO layer alongside JSON-LD): what the platform
/// is, where the machine-readable surfaces live.
pub async fn llms_txt(State(state): State<AppState>) -> Response {
    let base = &state.cfg.public_base_url;
    let body = format!(
        "# Auto.ml\n\n\
         > Reproducible research platform: every paper ships with a code repository pointer \
         (repo + pinned commit) and independently verified reproduction evidence. Trust comes \
         from verified-by-N — N independent runners reproducing consistent results on their own \
         hardware — not from platform endorsement. Review decisions pass a human gate.\n\n\
         ## Key pages\n\
         - [Papers]({base}/papers): published papers with review status and reproduction aggregates\n\
         - [Forum]({base}/forum): public discussion\n\n\
         ## Machine-readable\n\
         - [OpenAPI]({base}/openapi.json): full REST API specification\n\
         - [Sitemap]({base}/sitemap.xml)\n\
         - Paper pages embed ScholarlyArticle + SoftwareSourceCode JSON-LD (server-rendered)\n\n\
         ## Reproduce a paper\n\
         Install the automl CLI (`GET {base}/api/v1/cli/install`), then `automl reproduce run <paper-id>` \
         executes the paper's reproduce spec on your machine and submits signed evidence.\n"
    );
    ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], body).into_response()
}

pub async fn sitemap_xml(State(state): State<AppState>) -> Response {
    let base = &state.cfg.public_base_url;
    // Accepted papers are the indexable corpus (drafts/rejected carry noindex).
    let rows: Vec<(Uuid, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
        "SELECT id, updated_at FROM papers WHERE review_status = 'accepted'
         ORDER BY updated_at DESC LIMIT 5000",
    )
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n",
    );
    for path in ["/", "/papers", "/forum"] {
        xml.push_str(&format!("  <url><loc>{base}{path}</loc></url>\n"));
    }
    for (id, updated_at) in rows {
        xml.push_str(&format!(
            "  <url><loc>{base}/papers/{id}</loc><lastmod>{}</lastmod></url>\n",
            updated_at.format("%Y-%m-%d")
        ));
    }
    let threads: Vec<(Uuid, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
        "SELECT id, updated_at FROM forum_threads WHERE deleted_at IS NULL
         ORDER BY updated_at DESC LIMIT 1000",
    )
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();
    for (id, updated_at) in threads {
        xml.push_str(&format!(
            "  <url><loc>{base}/forum/threads/{id}</loc><lastmod>{}</lastmod></url>\n",
            updated_at.format("%Y-%m-%d")
        ));
    }
    xml.push_str("</urlset>\n");
    (
        [(header::CONTENT_TYPE, "application/xml; charset=utf-8")],
        xml,
    )
        .into_response()
}

/// `/papers/{uuid}` deep links get paper metadata injected into the shell.
/// Returns `None` for any other path (serve the shell untouched).
pub fn paper_id_from_path(path: &str) -> Option<Uuid> {
    let rest = path.strip_prefix("/papers/")?;
    if rest.contains('/') {
        return None;
    }
    Uuid::parse_str(rest).ok()
}

pub fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
    t.push('…');
    t
}

/// Inject title/description/OG/canonical + ScholarlyArticle JSON-LD into the
/// SPA shell for one paper. Non-accepted papers are marked noindex.
pub fn inject_paper_head(shell: &str, paper: &Paper, base: &str) -> String {
    let title = html_escape(&paper.title);
    let url = format!("{base}/papers/{}", paper.id);
    let description = html_escape(&truncate_chars(
        paper
            .abstract_text
            .lines()
            .collect::<Vec<_>>()
            .join(" ")
            .trim(),
        158,
    ));
    let accepted = paper.review_status == crate::domain::papers::ReviewStatus::Accepted;

    let authors: Vec<serde_json::Value> = paper
        .authors
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str())
                .map(|name| serde_json::json!({"@type": "Person", "name": name}))
                .collect()
        })
        .unwrap_or_default();

    let mut article = serde_json::json!({
        "@type": "ScholarlyArticle",
        "@id": url,
        "headline": paper.title,
        "url": url,
        "datePublished": paper.created_at.format("%Y-%m-%d").to_string(),
        "dateModified": paper.updated_at.format("%Y-%m-%d").to_string(),
        "isAccessibleForFree": true,
        "publisher": {"@type": "Organization", "name": "Auto.ml", "url": base},
    });
    if !paper.abstract_text.trim().is_empty() {
        article["abstract"] = serde_json::json!(paper.abstract_text);
    }
    if !authors.is_empty() {
        article["author"] = serde_json::json!(authors);
    }

    let breadcrumbs = serde_json::json!({
        "@type": "BreadcrumbList",
        "itemListElement": [
            {"@type": "ListItem", "position": 1, "name": "Home", "item": base},
            {"@type": "ListItem", "position": 2, "name": "Papers", "item": format!("{base}/papers")},
            {"@type": "ListItem", "position": 3, "name": paper.title, "item": url},
        ]
    });
    let mut graph = vec![article, breadcrumbs];
    if !paper.repo_url.is_empty() {
        let mut code = serde_json::json!({
            "@type": "SoftwareSourceCode",
            "name": format!("Reproduction code for: {}", paper.title),
            "codeRepository": paper.repo_url,
        });
        if !paper.commit_sha.is_empty() {
            code["version"] = serde_json::json!(paper.commit_sha);
        }
        graph.push(code);
    }
    // Escape <, >, & inside the JSON (valid JSON escapes) so hostile content
    // like a "</script>" title can never break out of the script element.
    let json_ld = serde_json::json!({"@context": "https://schema.org", "@graph": graph})
        .to_string()
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026");

    let mut head = String::new();
    head.push_str(&format!("<title>{title} — Auto.ml</title>\n"));
    head.push_str(&format!(
        "<meta name=\"description\" content=\"{description}\" />\n"
    ));
    if !accepted {
        head.push_str("<meta name=\"robots\" content=\"noindex\" />\n");
    }
    head.push_str(&format!("<link rel=\"canonical\" href=\"{url}\" />\n"));
    head.push_str(&format!(
        "<meta property=\"og:type\" content=\"article\" />\n\
         <meta property=\"og:site_name\" content=\"Auto.ml\" />\n\
         <meta property=\"og:title\" content=\"{title}\" />\n\
         <meta property=\"og:description\" content=\"{description}\" />\n\
         <meta property=\"og:url\" content=\"{url}\" />\n\
         <meta property=\"og:image\" content=\"{base}/og.png\" />\n\
         <meta name=\"twitter:card\" content=\"summary_large_image\" />\n"
    ));
    head.push_str(&format!(
        "<script type=\"application/ld+json\">{json_ld}</script>\n"
    ));

    inject_head(&strip_site_block(shell), &head)
}

/// Remove the marker-delimited site-level SEO block so a page carries exactly
/// one title/canonical/OG set.
fn strip_site_block(shell: &str) -> String {
    match (
        shell.find("<!-- seo:site -->"),
        shell.find("<!-- /seo:site -->"),
    ) {
        (Some(a), Some(b)) if b > a => format!(
            "{}{}",
            &shell[..a],
            &shell[b + "<!-- /seo:site -->".len()..]
        ),
        // Fallback: at least drop the static <title> to avoid duplicates.
        _ => match (shell.find("<title>"), shell.find("</title>")) {
            (Some(a), Some(b)) if b > a => {
                format!("{}{}", &shell[..a], &shell[b + "</title>".len()..])
            }
            _ => shell.to_string(),
        },
    }
}

fn inject_head(shell: &str, head: &str) -> String {
    shell.replace("</head>", &format!("{head}</head>"))
}

/// Put a crawler-visible content snapshot inside `#root`; the client app
/// replaces it on mount. Without this every page body is an empty div to
/// anything that does not run JavaScript.
pub fn inject_body(shell: &str, snapshot: &str) -> String {
    shell.replace(
        "<div id=\"root\"></div>",
        &format!("<div id=\"root\">{snapshot}</div>"),
    )
}

/// True for routes that get a server-rendered snapshot.
pub fn route_wants_snapshot(path: &str) -> bool {
    path == "/"
        || path == "/papers"
        || paper_id_from_path(path).is_some()
        || thread_id_from_path(path).is_some()
}

pub fn thread_id_from_path(path: &str) -> Option<Uuid> {
    let rest = path.strip_prefix("/forum/threads/")?;
    if rest.contains('/') {
        return None;
    }
    Uuid::parse_str(rest).ok()
}

/// Render the crawler-facing HTML for `path`, or `None` to serve the shell
/// untouched. Never fails the request: any lookup error falls back to `None`.
pub async fn render_route(state: &AppState, path: &str, shell: &str) -> Option<String> {
    let base = &state.cfg.public_base_url;

    if let Some(id) = paper_id_from_path(path) {
        let paper = crate::domain::papers::get(&state.db, id).await.ok()?;
        let summary = crate::domain::reproduce::reproduction_summary(&state.db, id)
            .await
            .ok();
        let with_head = inject_paper_head(shell, &paper, base);
        return Some(inject_body(
            &with_head,
            &paper_snapshot(&paper, summary.as_ref()),
        ));
    }

    if let Some(id) = thread_id_from_path(path) {
        let thread = crate::domain::forum::get_thread(&state.db, id).await.ok()?;
        let posts = crate::domain::forum::list_posts(
            &state.db,
            id,
            &automl_core::PageParams {
                limit: Some(20),
                cursor: None,
            },
        )
        .await
        .ok()?;
        let with_head = inject_thread_head(shell, &thread, &posts.data, base);
        return Some(inject_body(
            &with_head,
            &thread_snapshot(&thread, &posts.data),
        ));
    }

    if path == "/" || path == "/papers" {
        let rows: Vec<(Uuid, String, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
            "SELECT id, title, updated_at FROM papers WHERE review_status = 'accepted'
             ORDER BY updated_at DESC LIMIT 30",
        )
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();
        let shell = if path == "/papers" {
            let head = format!(
                "<title>Papers — Auto.ml</title>\n\
                 <meta name=\"description\" content=\"Published papers with code pointers and independently verified reproduction evidence (verified-by-N).\" />\n\
                 <link rel=\"canonical\" href=\"{base}/papers\" />\n\
                 <meta property=\"og:type\" content=\"website\" />\n\
                 <meta property=\"og:site_name\" content=\"Auto.ml\" />\n\
                 <meta property=\"og:title\" content=\"Papers — Auto.ml\" />\n\
                 <meta property=\"og:url\" content=\"{base}/papers\" />\n\
                 <meta property=\"og:image\" content=\"{base}/og.png\" />\n\
                 <meta name=\"twitter:card\" content=\"summary_large_image\" />\n"
            );
            inject_head(&strip_site_block(shell), &head)
        } else {
            shell.to_string()
        };
        return Some(inject_body(&shell, &index_snapshot(&rows, base)));
    }

    None
}

fn paper_snapshot(
    paper: &Paper,
    summary: Option<&crate::domain::reproduce::ReproductionSummary>,
) -> String {
    let title = html_escape(&paper.title);
    let authors = paper
        .authors
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str())
                .map(html_escape)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    let mut html = format!("<article><h1>{title}</h1>");
    if !authors.is_empty() {
        html.push_str(&format!("<p>{authors}</p>"));
    }
    html.push_str(&format!(
        "<p>Status: {} · Published {}</p>",
        paper.review_status.as_str(),
        paper.created_at.format("%Y-%m-%d")
    ));
    if !paper.abstract_text.trim().is_empty() {
        html.push_str(&format!(
            "<h2>Abstract</h2><p>{}</p>",
            html_escape(paper.abstract_text.trim())
        ));
    }
    html.push_str("<h2>Code</h2><dl>");
    if !paper.repo_url.is_empty() {
        html.push_str(&format!(
            "<dt>Repository</dt><dd><a href=\"{0}\">{0}</a></dd>",
            html_escape(&paper.repo_url)
        ));
    }
    if !paper.commit_sha.is_empty() {
        html.push_str(&format!(
            "<dt>Pinned commit</dt><dd>{}</dd>",
            html_escape(&paper.commit_sha)
        ));
    }
    html.push_str("</dl>");
    if let Some(s) = summary {
        html.push_str(&format!(
            "<h2>Reproduction</h2><p>verified-by-{} — {} runs from {} independent runners.</p>",
            s.verified_by_n, s.runs_total, s.distinct_runners
        ));
    }
    html.push_str(&format!(
        "<p>Reproduce this paper on your own hardware: <code>automl reproduce run {}</code></p></article>",
        paper.id
    ));
    html
}

fn inject_thread_head(
    shell: &str,
    thread: &crate::domain::forum::Thread,
    posts: &[crate::domain::forum::Post],
    base: &str,
) -> String {
    let title = html_escape(&thread.title);
    let url = format!("{base}/forum/threads/{}", thread.id);
    let excerpt = posts
        .first()
        .map(|p| truncate_chars(&p.body.replace('\n', " "), 158))
        .unwrap_or_default();
    let description = html_escape(&excerpt);

    let mut discussion = serde_json::json!({
        "@type": "DiscussionForumPosting",
        "@id": url,
        "headline": thread.title,
        "url": url,
        "datePublished": thread.created_at.format("%Y-%m-%d").to_string(),
        "dateModified": thread.updated_at.format("%Y-%m-%d").to_string(),
    });
    if let Some(first) = posts.first() {
        discussion["text"] = serde_json::json!(first.body);
    }
    if posts.len() > 1 {
        discussion["commentCount"] = serde_json::json!(posts.len() - 1);
    }
    let breadcrumbs = serde_json::json!({
        "@type": "BreadcrumbList",
        "itemListElement": [
            {"@type": "ListItem", "position": 1, "name": "Home", "item": base},
            {"@type": "ListItem", "position": 2, "name": "Forum", "item": format!("{base}/forum")},
            {"@type": "ListItem", "position": 3, "name": thread.title, "item": url},
        ]
    });
    let json_ld =
        serde_json::json!({"@context": "https://schema.org", "@graph": [discussion, breadcrumbs]})
            .to_string()
            .replace('<', "\\u003c")
            .replace('>', "\\u003e")
            .replace('&', "\\u0026");

    let head = format!(
        "<title>{title} — Auto.ml Forum</title>\n\
         <meta name=\"description\" content=\"{description}\" />\n\
         <link rel=\"canonical\" href=\"{url}\" />\n\
         <meta property=\"og:type\" content=\"article\" />\n\
         <meta property=\"og:site_name\" content=\"Auto.ml\" />\n\
         <meta property=\"og:title\" content=\"{title}\" />\n\
         <meta property=\"og:description\" content=\"{description}\" />\n\
         <meta property=\"og:url\" content=\"{url}\" />\n\
         <meta property=\"og:image\" content=\"{base}/og.png\" />\n\
         <meta name=\"twitter:card\" content=\"summary_large_image\" />\n\
         <script type=\"application/ld+json\">{json_ld}</script>\n"
    );
    inject_head(&strip_site_block(shell), &head)
}

fn thread_snapshot(
    thread: &crate::domain::forum::Thread,
    posts: &[crate::domain::forum::Post],
) -> String {
    let mut html = format!("<article><h1>{}</h1>", html_escape(&thread.title));
    for p in posts {
        html.push_str(&format!(
            "<section><p>{}</p><p><small>{}</small></p></section>",
            html_escape(&p.body),
            p.created_at.format("%Y-%m-%d")
        ));
    }
    html.push_str("</article>");
    html
}

fn index_snapshot(rows: &[(Uuid, String, chrono::DateTime<chrono::Utc>)], base: &str) -> String {
    let mut html = String::from(
        "<header><h1>Auto.ml</h1>\
         <p>A reproducible research platform: every paper ships with a code repository pointer \
         and independently verified reproduction evidence. Trust comes from verified-by-N — \
         independent runners reproducing consistent results — with review decisions passing a \
         human gate.</p></header><main><h2>Recent accepted papers</h2>",
    );
    if rows.is_empty() {
        html.push_str("<p>No accepted papers yet.</p>");
    } else {
        html.push_str("<ul>");
        for (id, title, updated) in rows {
            html.push_str(&format!(
                "<li><a href=\"{base}/papers/{id}\">{}</a> — {}</li>",
                html_escape(title),
                updated.format("%Y-%m-%d")
            ));
        }
        html.push_str("</ul>");
    }
    html.push_str("</main>");
    html
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::papers::ReviewStatus;
    use chrono::Utc;

    fn paper(status: ReviewStatus) -> Paper {
        Paper {
            id: Uuid::nil(),
            title: "Attention <is> all & you need".into(),
            abstract_text: "A study.\nMultiline abstract.".into(),
            authors: serde_json::json!(["Ada Lovelace", "Alan Turing"]),
            repo_url: "https://code.auto.ml/papers/x.git".into(),
            commit_sha: "a".repeat(40),
            latest_commit: None,
            reproduce_path: "reproduce.yaml".into(),
            code_status: "ready".into(),
            review_status: status,
            submitted_by: Uuid::nil(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn paper_path_matcher() {
        let id = Uuid::new_v4();
        assert_eq!(paper_id_from_path(&format!("/papers/{id}")), Some(id));
        assert_eq!(paper_id_from_path("/papers/not-a-uuid"), None);
        assert_eq!(paper_id_from_path(&format!("/papers/{id}/x")), None);
        assert_eq!(paper_id_from_path("/forum"), None);
    }

    #[test]
    fn injects_escaped_meta_and_schema() {
        let shell = "<html><head><!-- seo:site --><title>Auto.ml</title>\
<link rel=\"canonical\" href=\"https://auto.ml/\" /><!-- /seo:site --></head><body></body></html>";
        let html = inject_paper_head(shell, &paper(ReviewStatus::Accepted), "https://auto.ml");
        assert!(html.contains("Attention &lt;is&gt; all &amp; you need — Auto.ml"));
        assert!(
            !html.contains("<title>Auto.ml</title>"),
            "static title replaced"
        );
        assert!(html.contains("application/ld+json"));
        assert!(html.contains("ScholarlyArticle"));
        assert!(html.contains("SoftwareSourceCode"));
        assert!(html.contains("Ada Lovelace"));
        assert!(html.contains("rel=\"canonical\""));
        assert!(
            !html.contains("href=\"https://auto.ml/\""),
            "site-level canonical removed — exactly one canonical per page"
        );
        assert!(!html.contains("noindex"), "accepted papers are indexable");
        // Raw title must never appear unescaped in an attribute/text position.
        assert!(!html.contains("Attention <is>"));
    }

    #[test]
    fn non_accepted_papers_are_noindex() {
        let shell = "<html><head><title>Auto.ml</title></head><body></body></html>";
        let html = inject_paper_head(shell, &paper(ReviewStatus::Draft), "https://auto.ml");
        assert!(html.contains("noindex"));
    }
}
