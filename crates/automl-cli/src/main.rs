//! Auto.ml CLI — the client-side execution engine.
//!
//! Credential precedence: `--api-key` / `AUTOML_API_KEY` > saved
//! `~/.config/automl/credentials.toml` (written by `automl login`, 0600).
//! `--format json` is the stable machine interface for every command.

mod client;
mod cmds;
mod creds;
mod login;
mod output;
mod repro;

use std::{path::PathBuf, time::Duration};

use clap::{ArgGroup, Parser, Subcommand, ValueEnum};
use serde_json::Value;

use client::Api;
use output::{print_value, Format};

const USER_AGENT: &str = concat!("automl-cli/", env!("CARGO_PKG_VERSION"));

#[derive(Parser)]
#[command(name = "automl", version, about = "Auto.ml command-line client")]
struct Cli {
    /// API server base URL.
    #[arg(
        long,
        global = true,
        env = "AUTOML_SERVER",
        default_value = "https://auto.ml"
    )]
    server: String,

    /// API key (`amk_…`). Overrides the saved login credential.
    #[arg(long, global = true, env = "AUTOML_API_KEY")]
    api_key: Option<String>,

    /// Output format; `json` is the stable machine interface.
    #[arg(long, global = true, value_enum, default_value_t = Format::Table)]
    format: Format,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Check server health.
    Health,
    /// Log in via the browser (W-ID SSO, loopback + PKCE) and save credentials.
    Login {
        /// No-browser fallback: read an existing amk_… key from stdin.
        #[arg(long)]
        with_api_key: bool,
    },
    /// Remove saved credentials (revokes the stored key best-effort).
    Logout,
    /// Papers.
    Papers {
        #[command(subcommand)]
        action: PapersCmd,
    },
    /// Research Hub projects.
    Project {
        #[command(subcommand)]
        action: ProjectCmd,
    },
    /// Reproduce runs.
    Runs {
        #[command(subcommand)]
        action: RunsCmd,
    },
    /// API keys.
    Keys {
        #[command(subcommand)]
        action: KeysCmd,
    },
    /// Reproduce a paper on this machine and submit evidence.
    Reproduce {
        #[command(subcommand)]
        action: ReproduceCmd,
    },
    /// Device key management (ed25519 signing identity).
    Device {
        #[command(subcommand)]
        action: DeviceCmd,
    },
    /// Update this binary from the platform's published artifacts.
    SelfUpdate,
}

#[derive(Subcommand)]
enum PapersCmd {
    /// List papers (cursor pagination).
    List {
        #[arg(long, default_value_t = 50)]
        limit: u32,
        /// Follow next_cursor to exhaustion.
        #[arg(long)]
        all: bool,
    },
    /// Show one paper.
    Get { id: String },
    /// Submit a draft paper for review.
    Submit { id: String },
}

#[derive(Subcommand)]
enum ProjectCmd {
    /// Create a research project (you become its owner).
    Create {
        /// URL slug (owner-unique, lowercase); derived from --title if omitted.
        #[arg(long)]
        slug: Option<String>,
        /// Project title.
        #[arg(long)]
        title: String,
        /// Short summary.
        #[arg(long, default_value = "")]
        summary: String,
        /// private | lab | public_meta
        #[arg(long, default_value = "private")]
        visibility: String,
    },
    /// List projects you belong to (cursor pagination).
    List {
        #[arg(long, default_value_t = 50)]
        limit: u32,
        /// Follow next_cursor to exhaustion.
        #[arg(long)]
        all: bool,
    },
    /// Show one project.
    Get { id: String },
    /// Add a scholar member (W-ID handle/UUID) using an owner's delegated key.
    MemberAdd {
        /// Project UUID.
        project_id: String,
        /// Scholar W-ID handle or UUID.
        #[arg(long)]
        scholar: Option<String>,
        /// Reserved agent UUID; P0 rejects agent enrollment with 501.
        #[arg(long)]
        agent: Option<String>,
        /// owner | member | viewer (scholar memberships only in P0).
        #[arg(long, default_value = "member")]
        role: String,
    },
    /// Add a paper pointer to a project.
    #[command(group(ArgGroup::new("primary").required(true).args([
        "doi",
        "arxiv_id",
        "w_papers_work_id",
        "openalex_id",
        "daas_document_id",
    ])))]
    PaperAdd {
        /// Project UUID.
        project_id: String,
        /// DOI identifier.
        #[arg(long)]
        doi: Option<String>,
        /// arXiv identifier (for example 2401.01234).
        #[arg(long)]
        arxiv_id: Option<String>,
        /// W Papers work identifier.
        #[arg(long)]
        w_papers_work_id: Option<String>,
        /// OpenAlex work identifier.
        #[arg(long)]
        openalex_id: Option<String>,
        /// DaaS document identifier.
        #[arg(long)]
        daas_document_id: Option<String>,
        /// Fuel role for this paper.
        #[arg(long, value_enum)]
        fuel_role: Option<FuelRole>,
        /// Notes about why this paper is in the project.
        #[arg(long)]
        notes: Option<String>,
        /// Comma-separated tags.
        #[arg(long, value_delimiter = ',')]
        tags: Vec<String>,
    },
    /// Bind the project to a W-DaaS workspace (project owner only).
    BindDaas {
        /// Project UUID.
        project_id: String,
        /// Downstream W-DaaS workspace identifier.
        #[arg(long)]
        workspace_id: String,
    },
    /// Retire the project's active W-DaaS workspace binding (project owner).
    UnbindDaas {
        /// Project UUID.
        project_id: String,
    },
    /// Show the project's active W-DaaS workspace binding.
    Binding {
        /// Project UUID.
        project_id: String,
    },
    /// Register a project asset pointer or a metadata-only note.
    AssetAdd {
        /// Project UUID.
        project_id: String,
        /// Asset kind.
        #[arg(long, value_enum)]
        kind: AssetKind,
        /// Display title.
        #[arg(long)]
        title: Option<String>,
        /// External URI pointer (AutoML/w-storage object key or URL).
        #[arg(long)]
        uri: Option<String>,
        /// W-DaaS artifact pointer.
        #[arg(long)]
        daas_artifact_id: Option<String>,
        /// Optional JSON object payload.
        #[arg(long)]
        metadata: Option<String>,
    },
    /// List project assets (cursor pagination).
    Assets {
        /// Project UUID.
        project_id: String,
        /// Filter by asset kind.
        #[arg(long, value_enum)]
        kind: Option<AssetKind>,
        #[arg(long, default_value_t = 50)]
        limit: u32,
        /// Follow next_cursor to exhaustion.
        #[arg(long)]
        all: bool,
    },
    /// Show one project asset.
    AssetGet {
        /// Project UUID.
        project_id: String,
        /// Asset UUID.
        asset_id: String,
    },
    /// Soft-delete a project asset.
    AssetRm {
        /// Project UUID.
        project_id: String,
        /// Asset UUID.
        asset_id: String,
    },
    /// Register a PDF as a DaaS DocumentAsset on the project shelf.
    ImportPdf {
        /// Project UUID.
        project_id: String,
        /// Document label (sent to DaaS; cached as the display title).
        #[arg(long)]
        title: Option<String>,
        /// Source URI when the client already uploaded the bytes elsewhere.
        #[arg(long)]
        uri: Option<String>,
        /// Pre-registered DaaS document id — skips the upstream import call.
        #[arg(long)]
        daas_document_id: Option<String>,
        /// Fuel role for this paper.
        #[arg(long, value_enum)]
        fuel_role: Option<FuelRole>,
        /// Notes about why this paper is in the project.
        #[arg(long)]
        notes: Option<String>,
        /// Comma-separated tags.
        #[arg(long, value_delimiter = ',')]
        tags: Vec<String>,
    },
    /// Run project hybrid search via the bound DaaS workspace.
    Search {
        /// Project UUID.
        project_id: String,
        /// Search query.
        #[arg(long)]
        query: String,
        /// Max hits (1-50, default 10).
        #[arg(long)]
        limit: Option<u32>,
        /// Persist the outcome as a search_snapshot asset (Member+).
        #[arg(long)]
        persist: bool,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
#[value(rename_all = "snake_case")]
enum FuelRole {
    Seed,
    Related,
    Baseline,
    CitedBy,
    Reading,
}

impl FuelRole {
    fn as_str(self) -> &'static str {
        match self {
            Self::Seed => "seed",
            Self::Related => "related",
            Self::Baseline => "baseline",
            Self::CitedBy => "cited_by",
            Self::Reading => "reading",
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
#[value(rename_all = "snake_case")]
enum AssetKind {
    Note,
    SearchSnapshot,
    KgSlice,
    ResearchDraft,
    ExperimentSpec,
    SimConfig,
    ResultBundle,
    Figure,
    NotebookPtr,
}

impl AssetKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Note => "note",
            Self::SearchSnapshot => "search_snapshot",
            Self::KgSlice => "kg_slice",
            Self::ResearchDraft => "research_draft",
            Self::ExperimentSpec => "experiment_spec",
            Self::SimConfig => "sim_config",
            Self::ResultBundle => "result_bundle",
            Self::Figure => "figure",
            Self::NotebookPtr => "notebook_ptr",
        }
    }
}

#[derive(Subcommand)]
enum RunsCmd {
    /// List reproduce runs.
    List {
        /// Filter by paper id.
        #[arg(long)]
        paper: Option<String>,
        #[arg(long, default_value_t = 50)]
        limit: u32,
        #[arg(long)]
        all: bool,
    },
    /// Show one run.
    Get { id: String },
}

#[derive(Subcommand)]
enum KeysCmd {
    /// List API keys.
    List {
        #[arg(long, default_value_t = 50)]
        limit: u32,
        #[arg(long)]
        all: bool,
    },
    /// Revoke a key by id.
    Revoke { id: String },
}

#[derive(Subcommand)]
enum ReproduceCmd {
    /// Clone paper pointer, run locally, sign + submit evidence.
    Run {
        /// Paper UUID.
        paper_id: String,
        /// Working directory for clone (default: temp).
        #[arg(long)]
        workdir: Option<PathBuf>,
        /// Skip git clone; use an existing checkout (testing).
        #[arg(long)]
        local_path: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum DeviceCmd {
    /// Generate an ed25519 keypair, register the public key, store private key 0600.
    Register {
        /// Agent UUID that will own this device.
        #[arg(long)]
        agent: String,
        /// Path for the private key file (default: ~/.config/automl/device.key).
        #[arg(long)]
        key_file: Option<PathBuf>,
        /// Optional environment fingerprint JSON object string.
        #[arg(long, default_value = "{}")]
        env_fingerprint: String,
    },
}

fn main() -> anyhow::Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let cli = Cli::parse();
    let http = w_http::build_blocking_client(
        reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(120))
            .user_agent(USER_AGENT),
    )?;
    let base = cli.server.trim_end_matches('/').to_string();

    // --api-key / env wins; otherwise the saved login credential (only when it
    // was issued for the same server — keys never leak across hosts).
    let api_key = cli.api_key.clone().or_else(|| {
        creds::load_credentials()
            .filter(|c| c.server == base)
            .map(|c| c.api_key)
    });
    let api = Api {
        base,
        http,
        api_key,
    };
    let fmt = cli.format;

    match cli.cmd {
        Cmd::Health => {
            let v: Value = api.get("/api/v1/health")?;
            print_value(&fmt, &v, |v| {
                format!(
                    "{} v{} — {} (database: {})",
                    v["service"].as_str().unwrap_or("?"),
                    v["version"].as_str().unwrap_or("?"),
                    v["status"].as_str().unwrap_or("?"),
                    v["checks"]["database"].as_str().unwrap_or("?"),
                )
            });
        }
        Cmd::Login { with_api_key } => {
            let v = login::login(&api, with_api_key)?;
            print_value(&fmt, &v, |v| {
                format!(
                    "logged in — key {} saved to {}",
                    v["key_id"].as_str().unwrap_or("(existing)"),
                    v["credentials"].as_str().unwrap_or("?"),
                )
            });
        }
        Cmd::Logout => {
            let v = login::logout(&api)?;
            print_value(&fmt, &v, |v| {
                v["status"].as_str().unwrap_or("?").to_string()
            });
        }
        Cmd::Papers { action } => match action {
            PapersCmd::List { limit, all } => cmds::papers_list(&api, &fmt, limit, all)?,
            PapersCmd::Get { id } => cmds::papers_get(&api, &fmt, &id)?,
            PapersCmd::Submit { id } => cmds::papers_submit(&api, &fmt, &id)?,
        },
        Cmd::Project { action } => match action {
            ProjectCmd::Create {
                slug,
                title,
                summary,
                visibility,
            } => cmds::project_create(&api, &fmt, slug, title, summary, visibility)?,
            ProjectCmd::List { limit, all } => cmds::project_list(&api, &fmt, limit, all)?,
            ProjectCmd::Get { id } => cmds::project_get(&api, &fmt, &id)?,
            ProjectCmd::MemberAdd {
                project_id,
                scholar,
                agent,
                role,
            } => cmds::project_member_add(&api, &fmt, &project_id, scholar, agent, &role)?,
            ProjectCmd::PaperAdd {
                project_id,
                doi,
                arxiv_id,
                w_papers_work_id,
                openalex_id,
                daas_document_id,
                fuel_role,
                notes,
                tags,
            } => cmds::project_paper_add(
                &api,
                &fmt,
                &project_id,
                doi,
                arxiv_id,
                w_papers_work_id,
                openalex_id,
                daas_document_id,
                fuel_role.map(FuelRole::as_str),
                notes,
                tags,
            )?,
            ProjectCmd::BindDaas {
                project_id,
                workspace_id,
            } => cmds::project_bind_daas(&api, &fmt, &project_id, &workspace_id)?,
            ProjectCmd::UnbindDaas { project_id } => {
                cmds::project_unbind_daas(&api, &fmt, &project_id)?
            }
            ProjectCmd::Binding { project_id } => cmds::project_binding(&api, &fmt, &project_id)?,
            ProjectCmd::AssetAdd {
                project_id,
                kind,
                title,
                uri,
                daas_artifact_id,
                metadata,
            } => cmds::project_asset_add(
                &api,
                &fmt,
                &project_id,
                kind.as_str(),
                title,
                uri,
                daas_artifact_id,
                metadata,
            )?,
            ProjectCmd::Assets {
                project_id,
                kind,
                limit,
                all,
            } => cmds::project_assets(
                &api,
                &fmt,
                &project_id,
                kind.map(AssetKind::as_str),
                limit,
                all,
            )?,
            ProjectCmd::AssetGet {
                project_id,
                asset_id,
            } => cmds::project_asset_get(&api, &fmt, &project_id, &asset_id)?,
            ProjectCmd::AssetRm {
                project_id,
                asset_id,
            } => cmds::project_asset_rm(&api, &fmt, &project_id, &asset_id)?,
            ProjectCmd::ImportPdf {
                project_id,
                title,
                uri,
                daas_document_id,
                fuel_role,
                notes,
                tags,
            } => cmds::project_import_pdf(
                &api,
                &fmt,
                &project_id,
                title,
                uri,
                daas_document_id,
                fuel_role.map(FuelRole::as_str),
                notes,
                tags,
            )?,
            ProjectCmd::Search {
                project_id,
                query,
                limit,
                persist,
            } => cmds::project_search(&api, &fmt, &project_id, &query, limit, persist)?,
        },
        Cmd::Runs { action } => match action {
            RunsCmd::List { paper, limit, all } => {
                cmds::runs_list(&api, &fmt, paper.as_deref(), limit, all)?
            }
            RunsCmd::Get { id } => cmds::runs_get(&api, &fmt, &id)?,
        },
        Cmd::Keys { action } => match action {
            KeysCmd::List { limit, all } => cmds::keys_list(&api, &fmt, limit, all)?,
            KeysCmd::Revoke { id } => cmds::keys_revoke(&api, &fmt, &id)?,
        },
        Cmd::Reproduce {
            action:
                ReproduceCmd::Run {
                    paper_id,
                    workdir,
                    local_path,
                },
        } => {
            let result = repro::reproduce_run(&api, &paper_id, workdir, local_path)?;
            print_value(&fmt, &result, |v| {
                format!(
                    "run {} → {} (aggregate: GET /papers/{{id}}/reproduction)",
                    v["run"]["id"].as_str().unwrap_or("?"),
                    v["run"]["status"].as_str().unwrap_or("?"),
                )
            });
        }
        Cmd::Device {
            action:
                DeviceCmd::Register {
                    agent,
                    key_file,
                    env_fingerprint,
                },
        } => {
            let result = repro::device_register(&api, &agent, key_file, &env_fingerprint)?;
            print_value(&fmt, &result, |v| {
                format!(
                    "device {} registered for agent {}; private key → {}",
                    v["device_id"].as_str().unwrap_or("?"),
                    v["agent_id"].as_str().unwrap_or("?"),
                    v["key_file"].as_str().unwrap_or("?"),
                )
            });
        }
        Cmd::SelfUpdate => cmds::self_update(&api, &fmt)?,
    }
    Ok(())
}

#[cfg(test)]
mod cli_tests {
    use super::Cli;
    use clap::Parser;

    #[test]
    fn project_paper_add_requires_exactly_one_identifier() {
        assert!(Cli::try_parse_from(["automl", "project", "paper-add", "project-id"]).is_err());
        assert!(Cli::try_parse_from([
            "automl",
            "project",
            "paper-add",
            "project-id",
            "--doi",
            "10.1234/example",
            "--arxiv-id",
            "2401.01234",
        ])
        .is_err());
        assert!(Cli::try_parse_from([
            "automl",
            "project",
            "paper-add",
            "project-id",
            "--doi",
            "10.1234/example",
            "--fuel-role",
            "cited_by",
            "--tags",
            "seed,transformer",
        ])
        .is_ok());
    }
}
