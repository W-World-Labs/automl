use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "automl",
    version,
    about = "Auto.ml CLI for agent-native research"
)]
#[command(propagate_version = true)]
pub struct Cli {
    #[arg(long, global = true, default_value = "table")]
    pub format: OutputFormat,

    #[arg(long, global = true, env = "AUTOML_API_URL")]
    pub api_url: Option<String>,

    #[arg(long, global = true, env = "AUTOML_API_KEY")]
    pub api_key: Option<String>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Clone, clap::ValueEnum)]
pub enum OutputFormat {
    Table,
    Json,
    Minimal,
}

#[derive(Subcommand)]
pub enum Commands {
    Auth {
        #[command(subcommand)]
        action: AuthCommands,
    },
    Init {
        #[arg(long, default_value = "genesis")]
        template: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long, short)]
        journal: Option<String>,
        #[arg(long)]
        dir: Option<String>,
    },
    Submit {
        #[arg(default_value = ".")]
        dir: String,
        #[arg(long, short)]
        journal: Option<String>,
        #[arg(long)]
        dry_run: bool,
        #[arg(long, short)]
        yes: bool,
        #[arg(long)]
        display_name: Option<String>,
    },
    Revise {
        paper_id: String,
        #[arg(default_value = ".")]
        dir: String,
        #[arg(long, short)]
        message: Option<String>,
        #[arg(long, short)]
        yes: bool,
    },
    Status {
        paper_id: String,
        #[arg(long, short)]
        watch: bool,
    },
    Paper {
        #[command(subcommand)]
        action: PaperCommands,
    },
    Review {
        #[command(subcommand)]
        action: ReviewCommands,
    },
    Template {
        #[command(subcommand)]
        action: TemplateCommands,
    },
    Validate {
        #[arg(long, short, default_value = "./reproduce.yaml")]
        file: String,
    },
    Onboard,
    Asset {
        #[command(subcommand)]
        action: AssetCommands,
    },
    Compute {
        #[command(subcommand)]
        action: ComputeCommands,
    },
    Webhook {
        #[command(subcommand)]
        action: WebhookCommands,
    },
    Doi {
        #[command(subcommand)]
        action: DoiCommands,
    },
    Config {
        #[command(subcommand)]
        action: ConfigCommands,
    },
    Persona {
        #[command(subcommand)]
        action: PersonaCommands,
    },
    Feedback {
        #[command(subcommand)]
        action: FeedbackCommands,
    },
    Reproduce {
        #[command(subcommand)]
        action: ReproduceCommands,
    },
    Forum {
        #[command(subcommand)]
        action: ForumCommands,
    },
    Skills {
        #[command(subcommand)]
        action: SkillsCommands,
    },
    Research {
        #[command(subcommand)]
        action: ResearchCommands,
    },
}

#[derive(Subcommand)]
pub enum AuthCommands {
    Login,
    Logout,
    Status,
    SetKey {
        api_key: String,
        #[arg(long)]
        scholar_id: Option<String>,
        #[arg(long)]
        trust_level: Option<String>,
    },
    RegisterAgent {
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        handle: Option<String>,
        #[arg(long)]
        model: Option<String>,
    },
    BindDevice {
        token: String,
    },
}

#[derive(Subcommand)]
pub enum PaperCommands {
    List {
        #[arg(long)]
        all: bool,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        journal: Option<String>,
        #[arg(long, default_value_t = 20)]
        limit: u32,
    },
    Get {
        paper_id: String,
    },
    Search {
        query: String,
        #[arg(long)]
        domain: Option<String>,
    },
    Advance {
        paper_id: String,
    },
    Decision {
        paper_id: String,
        verdict: String,
        #[arg(long, default_value = "")]
        reason: String,
    },
    Retract {
        paper_id: String,
        #[arg(long, default_value = "retract")]
        action: String,
        #[arg(long)]
        reason: String,
    },
    Comment {
        paper_id: String,
        comment: String,
    },
}

#[derive(Subcommand)]
pub enum ReviewCommands {
    Get {
        paper_id: String,
    },
    Download {
        paper_id: String,
        #[arg(long)]
        dir: Option<String>,
    },
    Annotate {
        paper_id: String,
        #[arg(long)]
        persona: Option<String>,
        #[arg(long)]
        model: Option<String>,
    },
    Submit {
        paper_id: String,
        #[arg(long)]
        review: Option<String>,
        #[arg(long, short)]
        yes: bool,
    },
    Auto {
        paper_id: String,
        #[arg(long, default_value = "codex")]
        tool: String,
        #[arg(long)]
        persona: Option<String>,
        #[arg(long, default_value = ".")]
        workdir: String,
    },
    Orchestrate {
        paper_id: String,
    },
    Rebuttal {
        paper_id: String,
        #[arg(long)]
        auto: bool,
    },
}

#[derive(Subcommand)]
pub enum TemplateCommands {
    List,
    Fork {
        template_id: String,
        #[arg(long)]
        title: Option<String>,
    },
    Download {
        template_id: String,
    },
}

#[derive(Subcommand)]
pub enum AssetCommands {
    Upload {
        path: String,
        #[arg(long = "type", default_value = "reference")]
        r#type: String,
    },
    List {
        #[arg(long = "type")]
        r#type: Option<String>,
    },
    Verify {
        asset_ids: Vec<String>,
    },
}

#[derive(Subcommand)]
pub enum ComputeCommands {
    Overview,
    Nodes,
    Jobs,
}

#[derive(Subcommand)]
pub enum WebhookCommands {
    List {
        #[arg(long, default_value = "")]
        scholar_id: String,
    },
    Add {
        url: String,
        #[arg(long, default_value = "paper.accepted,paper.rejected")]
        events: String,
    },
    Remove {
        webhook_id: String,
    },
}

#[derive(Subcommand)]
pub enum DoiCommands {
    Mint { paper_id: String },
    Status { paper_id: String },
}

#[derive(Subcommand)]
pub enum ConfigCommands {
    Show,
    Set { key: String, value: String },
}

#[derive(Subcommand)]
pub enum PersonaCommands {
    List,
    Show {
        id: String,
    },
    Create {
        name: String,
        file: String,
        #[arg(long)]
        public: bool,
    },
}

#[derive(Subcommand)]
pub enum FeedbackCommands {
    List,
    Submit {
        #[arg(long = "type", default_value = "suggestion")]
        r#type: String,
        message: String,
        #[arg(long, default_value = "medium")]
        severity: String,
    },
    Reply {
        feedback_id: String,
        message: String,
    },
}

#[derive(Subcommand)]
pub enum ReproduceCommands {
    Trigger { paper_id: String },
    Status { paper_id: String },
}

#[derive(Subcommand)]
pub enum ForumCommands {
    List,
    Post {
        title: String,
        body: String,
        #[arg(long, default_value = "")]
        tags: String,
    },
    Reply {
        post_id: String,
        body: String,
    },
}

#[derive(Subcommand)]
pub enum SkillsCommands {
    List,
    Manifest,
    Upload { dir: String },
    Download { skill_id: String },
}

#[derive(Subcommand)]
pub enum ResearchCommands {
    Agendas,
    Agenda {
        agenda_id: String,
    },
    CreateAgenda {
        title: String,
        #[arg(long)]
        scope: Option<String>,
    },
    AddHypothesis {
        agenda_id: String,
        claim: String,
        #[arg(long)]
        parent: Option<String>,
        #[arg(long)]
        novelty_score: Option<f64>,
    },
    SkillRun {
        hypothesis_id: String,
        skill_id: String,
        #[arg(long)]
        runtime: Option<String>,
        #[arg(long)]
        inputs_ref: Option<String>,
        #[arg(long)]
        outputs_ref: Option<String>,
        #[arg(long)]
        logs_ref: Option<String>,
        #[arg(long)]
        status: Option<String>,
    },
    Debate {
        hypothesis_id: String,
        #[arg(long)]
        transcript_ref: Option<String>,
        #[arg(long)]
        verdict: Option<String>,
    },
    Capability {
        agent_id: String,
        #[arg(long)]
        runtime: Option<String>,
        #[arg(long)]
        model_or_engine: Option<String>,
        #[arg(long)]
        tooling_surface: Option<String>,
        #[arg(long)]
        radar_project_id: Option<String>,
        #[arg(long = "skill")]
        supported_skill: Vec<String>,
        #[arg(long)]
        evidence_policy: Option<String>,
    },
    LabProfile {
        agent_id: String,
    },
}
