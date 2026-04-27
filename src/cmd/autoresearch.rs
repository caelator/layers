//! Layers-native autoresearch for filling task context gaps.
//!
//! Local pre-edit context packet preparation lives in `layers preflight`.
//! This command persists and refreshes task-scoped findings for the Layers context spine.

use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Utc};
use clap::Subcommand;
use layers_core::{ContextPacket, ContextSection, ContextWarning};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_json::json;

use std::collections::HashSet;
use std::path::PathBuf;
use uuid::Uuid;

use crate::config::workspace_root;
use crate::context_packet_compiler::{cited_item, source};

/// Nested commands for `layers autoresearch`.
#[derive(Debug, Clone, Subcommand)]
pub enum AutoresearchCommands {
    /// Show the autoresearch `SQLite` database path.
    DbPath,
    /// Manage research sources.
    Source {
        /// Source subcommand.
        #[command(subcommand)]
        command: SourceCommands,
    },
    /// Search stored research entries.
    Search {
        /// Search query.
        query: String,
        /// Maximum results to return.
        #[arg(short, long, default_value = "10")]
        limit: usize,
        /// Emit JSON.
        #[arg(long)]
        json: bool,
    },
    /// Prepare task-specific research context and identify missing context.
    Prepare {
        /// Agent task to prepare context for.
        task: String,
        /// Optional files, symbols, areas, or URLs the task should target.
        #[arg(short, long)]
        target: Vec<String>,
        /// Maximum findings to include.
        #[arg(short, long, default_value = "8")]
        limit: usize,
        /// Emit JSON.
        #[arg(long)]
        json: bool,
    },
    /// Manage monitoring profiles.
    Profile {
        /// Profile subcommand.
        #[command(subcommand)]
        command: ProfileCommands,
    },
    /// Run one scan cycle for active profiles or a selected profile.
    ScanOnce {
        /// Restrict scan to one profile.
        #[arg(long)]
        profile_id: Option<String>,
        /// Emit JSON.
        #[arg(long)]
        json: bool,
    },
}

/// Research source commands.
#[derive(Debug, Clone, Subcommand)]
pub enum SourceCommands {
    /// Add a source URL.
    Add {
        /// Source URL.
        url: String,
        /// Optional source title.
        #[arg(short, long)]
        title: Option<String>,
        /// Source type: paper, article, web, or book.
        #[arg(short, long, default_value = "web")]
        source_type: String,
    },
    /// List sources.
    List {
        /// Maximum sources to show.
        #[arg(short, long, default_value = "50")]
        limit: usize,
        /// Emit JSON.
        #[arg(long)]
        json: bool,
    },
}

/// Research profile commands.
#[derive(Debug, Clone, Subcommand)]
pub enum ProfileCommands {
    /// Create a monitoring profile.
    Create {
        /// Profile name.
        #[arg(long)]
        name: String,
        /// Comma-separated keywords.
        #[arg(long)]
        keywords: String,
        /// Comma-separated negative keywords.
        #[arg(long)]
        negative_keywords: Option<String>,
        /// Minimum score threshold.
        #[arg(long)]
        score_threshold: Option<f64>,
        /// Maximum LLM calls per scan. Stored for compatibility; keyword scoring is used now.
        #[arg(long)]
        max_llm_calls: Option<u32>,
        /// Emit JSON.
        #[arg(long)]
        json: bool,
    },
    /// List monitoring profiles.
    List {
        /// Emit JSON.
        #[arg(long)]
        json: bool,
    },
}

/// Dispatch `layers autoresearch` commands.
pub fn handle_autoresearch(command: &AutoresearchCommands) -> Result<()> {
    let store = AutoresearchStore::open_default()?;
    match command {
        AutoresearchCommands::DbPath => {
            println!("{}", store.path.display());
            Ok(())
        }
        AutoresearchCommands::Source { command } => handle_source(&store, command),
        AutoresearchCommands::Search { query, limit, json } => {
            let results = store.search_entries(query, *limit)?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&results)?);
            } else if results.is_empty() {
                println!("no entries found for \"{query}\"");
            } else {
                println!("found {} entry(ies):\n", results.len());
                for entry in &results {
                    println!("  [{:.2}] {}", entry.relevance_score, entry.id);
                    println!(
                        "  title  : {}",
                        entry.summary.as_deref().unwrap_or("(no summary)")
                    );
                    println!("  content: {}", truncate(&entry.content, 120));
                }
            }
            Ok(())
        }
        AutoresearchCommands::Prepare {
            task,
            target,
            limit,
            json,
        } => {
            let report = prepare_task_context(&store, task, target, *limit)?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("autoresearch prepare: {}", report.task);
                println!("  selected findings: {}", report.selected_findings.len());
                println!("  missing context  : {}", report.missing_context.len());
                for finding in &report.selected_findings {
                    println!("  - [{:.2}] {}", finding.relevance_score, finding.title);
                    println!("    why: {}", finding.selected_reason);
                    println!("    provenance: {}", finding.provenance);
                }
                if !report.missing_context.is_empty() {
                    println!("missing context:");
                    for item in &report.missing_context {
                        println!("  - {item}");
                    }
                }
                if !report.suggested_actions.is_empty() {
                    println!("suggested actions:");
                    for action in &report.suggested_actions {
                        println!("  - {action}");
                    }
                }
            }
            Ok(())
        }
        AutoresearchCommands::Profile { command } => handle_profile(&store, command),
        AutoresearchCommands::ScanOnce { profile_id, json } => {
            let run = run_scan_once(&store, profile_id.as_deref())?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&run)?);
            } else {
                println!("scan complete");
                println!("  profiles : {}", run.profiles_scanned);
                println!("  sources  : {}", run.sources_considered);
                println!("  entries  : {}", run.entries_created);
                println!("  matches  : {}", run.matches_created);
            }
            Ok(())
        }
    }
}

fn handle_source(store: &AutoresearchStore, command: &SourceCommands) -> Result<()> {
    match command {
        SourceCommands::Add {
            url,
            title,
            source_type,
        } => {
            let source = ResearchSource::new(
                url.clone(),
                title.clone().unwrap_or_else(|| url.clone()),
                SourceType::from_str(source_type),
            );
            store.insert_source(&source)?;
            println!("added source {}", source.id);
            println!("  url   : {}", source.url);
            println!("  title : {}", source.title);
            println!("  type  : {}", source.source_type.as_str());
            Ok(())
        }
        SourceCommands::List { limit, json } => {
            let sources = store.list_sources(*limit)?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&sources)?);
            } else if sources.is_empty() {
                println!("no sources stored");
            } else {
                for source in &sources {
                    println!(
                        "{}  {}  {}",
                        source.id,
                        source.source_type.as_str(),
                        source.title
                    );
                    println!("  {}", source.url);
                }
            }
            Ok(())
        }
    }
}

fn handle_profile(store: &AutoresearchStore, command: &ProfileCommands) -> Result<()> {
    match command {
        ProfileCommands::Create {
            name,
            keywords,
            negative_keywords,
            score_threshold,
            max_llm_calls,
            json,
        } => {
            let mut profile = ResearchProfile::new(name.clone(), split_csv(keywords));
            profile.negative_keywords = negative_keywords
                .as_deref()
                .map(split_csv)
                .unwrap_or_default();
            if let Some(threshold) = score_threshold {
                profile.score_threshold = threshold.clamp(0.0, 1.0);
            }
            if let Some(max_calls) = max_llm_calls {
                profile.max_llm_calls = *max_calls;
            }
            store.insert_profile(&profile)?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&profile)?);
            } else {
                println!("created profile {}", profile.id);
                println!("  name     : {}", profile.name);
                println!("  keywords : {}", profile.keywords.join(", "));
            }
            Ok(())
        }
        ProfileCommands::List { json } => {
            let profiles = store.list_profiles()?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&profiles)?);
            } else if profiles.is_empty() {
                println!("no profiles stored");
            } else {
                for profile in &profiles {
                    println!("{}  {}", profile.id, profile.name);
                    println!("  keywords : {}", profile.keywords.join(", "));
                    println!("  threshold: {:.2}", profile.score_threshold);
                }
            }
            Ok(())
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum SourceType {
    Paper,
    Article,
    Web,
    Book,
}

impl SourceType {
    fn as_str(self) -> &'static str {
        match self {
            Self::Paper => "paper",
            Self::Article => "article",
            Self::Web => "web",
            Self::Book => "book",
        }
    }

    fn from_str(value: &str) -> Self {
        match value {
            "paper" => Self::Paper,
            "article" => Self::Article,
            "book" => Self::Book,
            _ => Self::Web,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ResearchSource {
    id: String,
    url: String,
    title: String,
    source_type: SourceType,
    added_at: DateTime<Utc>,
}

impl ResearchSource {
    fn new(url: String, title: String, source_type: SourceType) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            url,
            title,
            source_type,
            added_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ResearchEntry {
    id: String,
    source_id: String,
    content: String,
    summary: Option<String>,
    tags: Vec<String>,
    relevance_score: f64,
    last_reread_at: Option<DateTime<Utc>>,
}

impl ResearchEntry {
    fn from_source(source: &ResearchSource, profile: &ResearchProfile, score: f64) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            source_id: source.id.clone(),
            content: format!(
                "{}\n{}\nkeywords: {}",
                source.title,
                source.url,
                profile.keywords.join(", ")
            ),
            summary: Some(source.title.clone()),
            tags: profile.keywords.clone(),
            relevance_score: score,
            last_reread_at: Some(Utc::now()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ResearchMatch {
    id: String,
    entry_id: String,
    profile_id: String,
    source_id: String,
    task_key: String,
    relevance_score: f64,
    selected_reason: String,
    excluded_reason: Option<String>,
    freshness: String,
    reliability: String,
    matched_at: DateTime<Utc>,
}

impl ResearchMatch {
    fn from_entry_source_profile(
        entry: &ResearchEntry,
        source: &ResearchSource,
        profile: &ResearchProfile,
        score: f64,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            entry_id: entry.id.clone(),
            profile_id: profile.id.clone(),
            source_id: source.id.clone(),
            task_key: task_key_for_profile(profile),
            relevance_score: score,
            selected_reason: format!(
                "Matched profile '{}' keywords: {}",
                profile.name,
                matched_keywords(source, profile).join(", ")
            ),
            excluded_reason: None,
            freshness: freshness_label(entry.last_reread_at),
            reliability: reliability_label(source),
            matched_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ResearchProfile {
    id: String,
    name: String,
    keywords: Vec<String>,
    negative_keywords: Vec<String>,
    sources: Vec<String>,
    scoring_prompt: Option<String>,
    score_threshold: f64,
    max_llm_calls: u32,
    revision: u32,
    last_seen_at: Option<DateTime<Utc>>,
    archived_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

impl ResearchProfile {
    fn new(name: String, keywords: Vec<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            name,
            keywords,
            negative_keywords: Vec::new(),
            sources: Vec::new(),
            scoring_prompt: None,
            score_threshold: 0.5,
            max_llm_calls: 10,
            revision: 1,
            last_seen_at: None,
            archived_at: None,
            created_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ScanRunSummary {
    profiles_scanned: usize,
    sources_considered: usize,
    entries_created: usize,
    matches_created: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PreparedFinding {
    pub(crate) entry_id: String,
    pub(crate) source_id: String,
    pub(crate) title: String,
    pub(crate) content: String,
    pub(crate) relevance_score: f64,
    pub(crate) selected_reason: String,
    pub(crate) freshness: String,
    pub(crate) reliability: String,
    pub(crate) provenance: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PrepareReport {
    pub(crate) task: String,
    pub(crate) targets: Vec<String>,
    pub(crate) selected_findings: Vec<PreparedFinding>,
    pub(crate) excluded_findings: Vec<String>,
    pub(crate) missing_context: Vec<String>,
    pub(crate) suggested_actions: Vec<String>,
    pub(crate) open_uncertainty: Vec<String>,
    pub(crate) confidence: f64,
}

pub(crate) struct AutoresearchStore {
    path: PathBuf,
    conn: Connection,
}

impl AutoresearchStore {
    pub(crate) fn open_default() -> Result<Self> {
        let dir = workspace_root().join("memoryport");
        std::fs::create_dir_all(&dir)?;
        Self::open(dir.join("autoresearch.sqlite"))
    }

    #[cfg(test)]
    fn open_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let store = Self {
            path: PathBuf::from(":memory:"),
            conn,
        };
        store.migrate()?;
        Ok(store)
    }

    fn open(path: PathBuf) -> Result<Self> {
        let conn = Connection::open(&path).with_context(|| format!("open {}", path.display()))?;
        let store = Self { path, conn };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(
            r"
            CREATE TABLE IF NOT EXISTS autoresearch_sources (
                id TEXT PRIMARY KEY,
                url TEXT NOT NULL UNIQUE,
                title TEXT NOT NULL,
                source_type TEXT NOT NULL,
                added_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS autoresearch_entries (
                id TEXT PRIMARY KEY,
                source_id TEXT NOT NULL,
                content TEXT NOT NULL,
                summary TEXT,
                tags_json TEXT NOT NULL,
                relevance_score REAL NOT NULL,
                last_reread_at TEXT
            );
            CREATE TABLE IF NOT EXISTS autoresearch_profiles (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                keywords_json TEXT NOT NULL,
                negative_keywords_json TEXT NOT NULL,
                sources_json TEXT NOT NULL,
                scoring_prompt TEXT,
                score_threshold REAL NOT NULL,
                max_llm_calls INTEGER NOT NULL,
                revision INTEGER NOT NULL,
                last_seen_at TEXT,
                archived_at TEXT,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS autoresearch_matches (
                id TEXT PRIMARY KEY,
                entry_id TEXT NOT NULL,
                profile_id TEXT NOT NULL,
                source_id TEXT NOT NULL,
                task_key TEXT NOT NULL,
                relevance_score REAL NOT NULL,
                selected_reason TEXT NOT NULL,
                excluded_reason TEXT,
                freshness TEXT NOT NULL,
                reliability TEXT NOT NULL,
                matched_at TEXT NOT NULL,
                UNIQUE(entry_id, profile_id, task_key)
            );
            CREATE INDEX IF NOT EXISTS idx_autoresearch_entries_source
                ON autoresearch_entries(source_id);
            CREATE INDEX IF NOT EXISTS idx_autoresearch_entries_score
                ON autoresearch_entries(relevance_score DESC);
            CREATE INDEX IF NOT EXISTS idx_autoresearch_matches_profile
                ON autoresearch_matches(profile_id, relevance_score DESC);
            ",
        )?;
        Ok(())
    }

    fn insert_source(&self, source: &ResearchSource) -> Result<()> {
        self.conn.execute(
            "INSERT INTO autoresearch_sources (id, url, title, source_type, added_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![source.id, source.url, source.title, source.source_type.as_str(), source.added_at.to_rfc3339()],
        )?;
        Ok(())
    }

    fn list_sources(&self, limit: usize) -> Result<Vec<ResearchSource>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, url, title, source_type, added_at FROM autoresearch_sources ORDER BY added_at DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], row_to_source)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    fn insert_profile(&self, profile: &ResearchProfile) -> Result<()> {
        self.conn.execute(
            "INSERT INTO autoresearch_profiles (id, name, keywords_json, negative_keywords_json, sources_json, scoring_prompt, score_threshold, max_llm_calls, revision, last_seen_at, archived_at, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                profile.id,
                profile.name,
                serde_json::to_string(&profile.keywords)?,
                serde_json::to_string(&profile.negative_keywords)?,
                serde_json::to_string(&profile.sources)?,
                profile.scoring_prompt,
                profile.score_threshold,
                profile.max_llm_calls,
                profile.revision,
                profile.last_seen_at.map(|dt| dt.to_rfc3339()),
                profile.archived_at.map(|dt| dt.to_rfc3339()),
                profile.created_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    fn list_profiles(&self) -> Result<Vec<ResearchProfile>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, keywords_json, negative_keywords_json, sources_json, scoring_prompt, score_threshold, max_llm_calls, revision, last_seen_at, archived_at, created_at FROM autoresearch_profiles WHERE archived_at IS NULL ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map([], row_to_profile)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    fn insert_entry(&self, entry: &ResearchEntry) -> Result<()> {
        self.conn.execute(
            "INSERT INTO autoresearch_entries (id, source_id, content, summary, tags_json, relevance_score, last_reread_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                entry.id,
                entry.source_id,
                entry.content,
                entry.summary,
                serde_json::to_string(&entry.tags)?,
                entry.relevance_score,
                entry.last_reread_at.map(|dt| dt.to_rfc3339()),
            ],
        )?;
        Ok(())
    }

    fn entry_for_source(&self, source_id: &str) -> Result<Option<ResearchEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, source_id, content, summary, tags_json, relevance_score, last_reread_at
             FROM autoresearch_entries
             WHERE source_id = ?1
             ORDER BY relevance_score DESC, last_reread_at DESC
             LIMIT 1",
        )?;
        let mut rows = stmt.query_map(params![source_id], row_to_entry)?;
        rows.next().transpose().map_err(Into::into)
    }

    fn insert_match(&self, research_match: &ResearchMatch) -> Result<bool> {
        let inserted = self.conn.execute(
            "INSERT OR IGNORE INTO autoresearch_matches (id, entry_id, profile_id, source_id, task_key, relevance_score, selected_reason, excluded_reason, freshness, reliability, matched_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                research_match.id,
                research_match.entry_id,
                research_match.profile_id,
                research_match.source_id,
                research_match.task_key,
                research_match.relevance_score,
                research_match.selected_reason,
                research_match.excluded_reason,
                research_match.freshness,
                research_match.reliability,
                research_match.matched_at.to_rfc3339(),
            ],
        )?;
        Ok(inserted > 0)
    }

    #[cfg(test)]
    fn list_matches_for_profile(
        &self,
        profile_id: &str,
        limit: usize,
    ) -> Result<Vec<ResearchMatch>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, entry_id, profile_id, source_id, task_key, relevance_score, selected_reason, excluded_reason, freshness, reliability, matched_at
             FROM autoresearch_matches
             WHERE profile_id = ?1
             ORDER BY relevance_score DESC, matched_at DESC
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![profile_id, limit], row_to_match)?;
        collect_rows(rows)
    }

    fn search_entries(&self, query: &str, limit: usize) -> Result<Vec<ResearchEntry>> {
        let pattern = format!("%{}%", query.replace('%', "\\%"));
        let mut stmt = self.conn.prepare(
            "SELECT id, source_id, content, summary, tags_json, relevance_score, last_reread_at
             FROM autoresearch_entries
             WHERE content LIKE ?1 ESCAPE '\\' OR summary LIKE ?1 ESCAPE '\\'
             ORDER BY relevance_score DESC, last_reread_at DESC
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![pattern, limit], row_to_entry)?;
        collect_rows(rows)
    }
}

pub(crate) struct AutoresearchPacketBridgeOptions<'a> {
    pub(crate) task: &'a str,
    pub(crate) targets: &'a [String],
    pub(crate) limit: usize,
    pub(crate) unavailable_message: &'a str,
}

pub(crate) fn add_autoresearch_to_packet(
    packet: &mut ContextPacket,
    options: AutoresearchPacketBridgeOptions<'_>,
) -> usize {
    let Ok(store) = AutoresearchStore::open_default() else {
        packet.warnings.push(ContextWarning {
            severity: "info".to_string(),
            code: "autoresearch_unavailable".to_string(),
            message: options.unavailable_message.to_string(),
        });
        return 0;
    };
    let Ok(report) = prepare_task_context(&store, options.task, options.targets, options.limit)
    else {
        packet.warnings.push(ContextWarning {
            severity: "warning".to_string(),
            code: "autoresearch_prepare_failed".to_string(),
            message: "Persisted autoresearch findings could not be prepared for this task."
                .to_string(),
        });
        return 0;
    };

    for uncertainty in &report.open_uncertainty {
        packet.warnings.push(ContextWarning {
            severity: "info".to_string(),
            code: "autoresearch_uncertainty".to_string(),
            message: uncertainty.clone(),
        });
    }
    packet
        .open_uncertainty
        .extend(report.open_uncertainty.clone());

    let Some(section) = findings_to_context_section(&report.selected_findings) else {
        update_autoresearch_score(packet, 0);
        return 0;
    };
    let count = section.items.len();
    packet.sections.push(section);

    update_autoresearch_score(packet, count);
    count
}

pub(crate) fn findings_to_context_section(findings: &[PreparedFinding]) -> Option<ContextSection> {
    if findings.is_empty() {
        return None;
    }

    let items = findings
        .iter()
        .enumerate()
        .map(|(idx, finding)| {
            let body = format!(
                "{}\n\nFreshness: {}\nReliability: {}\nProvenance: {}",
                finding.content, finding.freshness, finding.reliability, finding.provenance
            );
            cited_item(
                format!("autoresearch-{}", idx + 1),
                finding.title.clone(),
                body,
                source("autoresearch", finding.provenance.clone()),
                finding.selected_reason.clone(),
                vec!["autoresearch".to_string(), "provenance".to_string()],
            )
            .with_score(Some(finding.relevance_score as f32))
            .with_token_estimate(finding.content.split_whitespace().count())
        })
        .collect::<Vec<_>>();

    Some(ContextSection {
        id: "autoresearch".to_string(),
        title: "Persisted Autoresearch Findings".to_string(),
        summary: Some(
            "Task-matched findings persisted by layers autoresearch, including provenance/freshness semantics."
                .to_string(),
        ),
        items,
    })
}

fn update_autoresearch_score(packet: &mut ContextPacket, count: usize) {
    if !packet.scores.is_object() {
        packet.scores = json!({});
    }
    if let Some(scores) = packet.scores.as_object_mut() {
        scores.insert("autoresearch_findings".to_string(), json!(count));
    }
}

pub(crate) trait AutoresearchProvider {
    fn search_findings(&self, query: &str, limit: usize) -> Result<Vec<ResearchEntry>>;
}

impl AutoresearchProvider for AutoresearchStore {
    fn search_findings(&self, query: &str, limit: usize) -> Result<Vec<ResearchEntry>> {
        self.search_entries(query, limit)
    }
}

pub(crate) fn prepare_findings(
    provider: &impl AutoresearchProvider,
    task: &str,
    targets: &[String],
    limit: usize,
) -> Result<PrepareReport> {
    let mut keywords = task_keywords(task);
    keywords.extend(targets.iter().flat_map(|target| task_keywords(target)));
    keywords.sort();
    keywords.dedup();

    let entries = search_entries_for_keywords(provider, task, &keywords, limit)?;
    let selected_findings = entries
        .iter()
        .map(prepared_finding_from_entry)
        .collect::<Vec<_>>();
    let mut missing_context = Vec::new();
    if selected_findings.is_empty() {
        missing_context.push("No persisted autoresearch finding matched this task.".to_string());
    }
    if targets.is_empty() {
        missing_context.push(
            "No explicit target was provided; source selection may miss the relevant subsystem."
                .to_string(),
        );
    }
    let mut suggested_actions = Vec::new();
    if selected_findings.is_empty() {
        suggested_actions.push(format!(
            "Seed a source or profile for task keywords: {}",
            if keywords.is_empty() {
                task.to_string()
            } else {
                keywords.join(", ")
            }
        ));
    }
    if !targets.is_empty() {
        suggested_actions.push(format!(
            "Bridge selected findings into ContextPacket sections for targets: {}",
            targets.join(", ")
        ));
    }
    let open_uncertainty = missing_context.clone();
    let confidence = if selected_findings.is_empty() {
        0.35
    } else {
        0.75
    };
    Ok(PrepareReport {
        task: task.to_string(),
        targets: targets.to_vec(),
        selected_findings,
        excluded_findings: Vec::new(),
        missing_context,
        suggested_actions,
        open_uncertainty,
        confidence,
    })
}

pub(crate) fn prepare_task_context(
    store: &AutoresearchStore,
    task: &str,
    targets: &[String],
    limit: usize,
) -> Result<PrepareReport> {
    prepare_findings(store, task, targets, limit)
}

fn search_entries_for_keywords(
    provider: &impl AutoresearchProvider,
    task: &str,
    keywords: &[String],
    limit: usize,
) -> Result<Vec<ResearchEntry>> {
    let terms = if keywords.is_empty() {
        vec![task.to_string()]
    } else {
        keywords.to_vec()
    };
    let mut seen = HashSet::new();
    let mut entries = Vec::new();
    for term in terms {
        for entry in provider.search_findings(&term, limit)? {
            if seen.insert(entry.id.clone()) {
                entries.push(entry);
            }
            if entries.len() >= limit {
                return Ok(entries);
            }
        }
    }
    Ok(entries)
}

fn prepared_finding_from_entry(entry: &ResearchEntry) -> PreparedFinding {
    PreparedFinding {
        entry_id: entry.id.clone(),
        source_id: entry.source_id.clone(),
        title: entry
            .summary
            .clone()
            .unwrap_or_else(|| "Untitled autoresearch finding".to_string()),
        content: entry.content.clone(),
        relevance_score: entry.relevance_score,
        selected_reason: format!(
            "Selected because stored finding text matched task context tags: {}",
            entry.tags.join(", ")
        ),
        freshness: freshness_label(entry.last_reread_at),
        reliability: "medium".to_string(),
        provenance: format!(
            "autoresearch entry {} from source {}",
            entry.id, entry.source_id
        ),
    }
}

fn task_keywords(value: &str) -> Vec<String> {
    value
        .split(|ch: char| !ch.is_alphanumeric() && ch != '_' && ch != '-')
        .map(|word| word.trim().to_lowercase())
        .filter(|word| word.len() >= 3)
        .filter(|word| {
            !matches!(
                word.as_str(),
                "the" | "and" | "for" | "with" | "into" | "this" | "that" | "from"
            )
        })
        .collect()
}

fn run_scan_once(store: &AutoresearchStore, profile_id: Option<&str>) -> Result<ScanRunSummary> {
    let profiles = store.list_profiles()?;
    let selected = profiles
        .into_iter()
        .filter(|profile| profile_id.is_none_or(|wanted| profile.id == wanted))
        .collect::<Vec<_>>();
    if profile_id.is_some() && selected.is_empty() {
        return Err(anyhow!("profile not found"));
    }

    let sources = store.list_sources(10_000)?;
    let mut entries_created = 0usize;
    let mut matches_created = 0usize;
    for profile in &selected {
        for source in &sources {
            if excluded_by_negative_keywords(source, profile) {
                continue;
            }
            let relevance = score_source(source, profile);
            if relevance <= 0.0 {
                continue;
            }
            let entry = if let Some(entry) = store.entry_for_source(&source.id)? {
                entry
            } else {
                let entry = ResearchEntry::from_source(source, profile, relevance);
                store.insert_entry(&entry)?;
                entries_created += 1;
                entry
            };
            if relevance >= profile.score_threshold {
                let research_match =
                    ResearchMatch::from_entry_source_profile(&entry, source, profile, relevance);
                if store.insert_match(&research_match)? {
                    matches_created += 1;
                }
            }
        }
    }
    Ok(ScanRunSummary {
        profiles_scanned: selected.len(),
        sources_considered: sources.len(),
        entries_created,
        matches_created,
    })
}

fn matched_keywords(source: &ResearchSource, profile: &ResearchProfile) -> Vec<String> {
    let haystack = format!("{} {}", source.title, source.url).to_lowercase();
    profile
        .keywords
        .iter()
        .filter(|keyword| haystack.contains(&keyword.to_lowercase()))
        .cloned()
        .collect()
}

fn task_key_for_profile(profile: &ResearchProfile) -> String {
    let mut keywords = profile
        .keywords
        .iter()
        .map(|keyword| keyword.trim().to_lowercase())
        .filter(|keyword| !keyword.is_empty())
        .collect::<Vec<_>>();
    keywords.sort();
    format!("profile:{}:{}", profile.id, keywords.join("|"))
}

fn freshness_label(last_reread_at: Option<DateTime<Utc>>) -> String {
    match last_reread_at {
        Some(read_at) if Utc::now().signed_duration_since(read_at).num_days() <= 7 => {
            "fresh".to_string()
        }
        Some(_) => "stale".to_string(),
        None => "unknown".to_string(),
    }
}

fn reliability_label(source: &ResearchSource) -> String {
    match source.source_type {
        SourceType::Paper | SourceType::Book => "high".to_string(),
        SourceType::Article | SourceType::Web => "medium".to_string(),
    }
}

fn score_source(source: &ResearchSource, profile: &ResearchProfile) -> f64 {
    let haystack = format!("{} {}", source.title, source.url).to_lowercase();
    let hits = profile
        .keywords
        .iter()
        .filter(|keyword| haystack.contains(&keyword.to_lowercase()))
        .count();
    if profile.keywords.is_empty() {
        0.0
    } else {
        (hits as f64 / profile.keywords.len() as f64).clamp(0.0, 1.0)
    }
}

fn excluded_by_negative_keywords(source: &ResearchSource, profile: &ResearchProfile) -> bool {
    let haystack = format!("{} {}", source.title, source.url).to_lowercase();
    profile
        .negative_keywords
        .iter()
        .any(|keyword| haystack.contains(&keyword.to_lowercase()))
}

fn row_to_source(row: &rusqlite::Row<'_>) -> rusqlite::Result<ResearchSource> {
    let added_at: String = row.get(4)?;
    Ok(ResearchSource {
        id: row.get(0)?,
        url: row.get(1)?,
        title: row.get(2)?,
        source_type: SourceType::from_str(&row.get::<_, String>(3)?),
        added_at: parse_rfc3339_or_now(&added_at),
    })
}

fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<ResearchEntry> {
    let tags_json: String = row.get(4)?;
    let last_reread_at: Option<String> = row.get(6)?;
    Ok(ResearchEntry {
        id: row.get(0)?,
        source_id: row.get(1)?,
        content: row.get(2)?,
        summary: row.get(3)?,
        tags: serde_json::from_str(&tags_json).unwrap_or_default(),
        relevance_score: row.get(5)?,
        last_reread_at: last_reread_at.as_deref().map(parse_rfc3339_or_now),
    })
}

fn row_to_profile(row: &rusqlite::Row<'_>) -> rusqlite::Result<ResearchProfile> {
    let keywords_json: String = row.get(2)?;
    let negative_keywords_json: String = row.get(3)?;
    let sources_json: String = row.get(4)?;
    let last_seen_at: Option<String> = row.get(9)?;
    let archived_at: Option<String> = row.get(10)?;
    let created_at: String = row.get(11)?;
    Ok(ResearchProfile {
        id: row.get(0)?,
        name: row.get(1)?,
        keywords: serde_json::from_str(&keywords_json).unwrap_or_default(),
        negative_keywords: serde_json::from_str(&negative_keywords_json).unwrap_or_default(),
        sources: serde_json::from_str(&sources_json).unwrap_or_default(),
        scoring_prompt: row.get(5)?,
        score_threshold: row.get(6)?,
        max_llm_calls: row.get(7)?,
        revision: row.get(8)?,
        last_seen_at: last_seen_at.as_deref().map(parse_rfc3339_or_now),
        archived_at: archived_at.as_deref().map(parse_rfc3339_or_now),
        created_at: parse_rfc3339_or_now(&created_at),
    })
}

#[cfg(test)]
fn row_to_match(row: &rusqlite::Row<'_>) -> rusqlite::Result<ResearchMatch> {
    let matched_at: String = row.get(10)?;
    Ok(ResearchMatch {
        id: row.get(0)?,
        entry_id: row.get(1)?,
        profile_id: row.get(2)?,
        source_id: row.get(3)?,
        task_key: row.get(4)?,
        relevance_score: row.get(5)?,
        selected_reason: row.get(6)?,
        excluded_reason: row.get(7)?,
        freshness: row.get(8)?,
        reliability: row.get(9)?,
        matched_at: parse_rfc3339_or_now(&matched_at),
    })
}

fn collect_rows<T, I>(rows: I) -> Result<Vec<T>>
where
    I: Iterator<Item = rusqlite::Result<T>>,
{
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

fn parse_rfc3339_or_now(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value).map_or_else(|_| Utc::now(), |dt| dt.with_timezone(&Utc))
}

fn split_csv(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn truncate(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut truncated = value.chars().take(max_chars).collect::<String>();
    truncated.push('…');
    truncated
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_type_unknown_defaults_to_web() {
        assert_eq!(SourceType::from_str("unknown"), SourceType::Web);
    }

    #[test]
    fn profile_defaults_match_research_radar() {
        let profile = ResearchProfile::new("AI".to_string(), vec!["AI".to_string()]);
        assert!((profile.score_threshold - 0.5).abs() < f64::EPSILON);
        assert_eq!(profile.max_llm_calls, 10);
        assert_eq!(profile.revision, 1);
        assert!(profile.archived_at.is_none());
    }

    struct StaticProvider {
        entries: Vec<ResearchEntry>,
    }

    impl AutoresearchProvider for StaticProvider {
        fn search_findings(&self, _query: &str, _limit: usize) -> Result<Vec<ResearchEntry>> {
            Ok(self.entries.clone())
        }
    }

    struct UnavailableProvider;

    impl AutoresearchProvider for UnavailableProvider {
        fn search_findings(&self, _query: &str, _limit: usize) -> Result<Vec<ResearchEntry>> {
            Err(anyhow!("provider unavailable"))
        }
    }

    #[test]
    fn prepare_findings_reports_empty_context_without_packet_section() {
        let report = prepare_findings(
            &StaticProvider {
                entries: Vec::new(),
            },
            "compile context packets",
            &[],
            8,
        )
        .unwrap();

        assert!(report.selected_findings.is_empty());
        assert!(
            report
                .missing_context
                .iter()
                .any(|message| message.contains("No persisted autoresearch finding"))
        );
        assert!(findings_to_context_section(&report.selected_findings).is_none());
    }

    #[test]
    fn prepare_findings_propagates_unavailable_provider() {
        let err = prepare_findings(
            &UnavailableProvider,
            "compile context packets",
            &["src/cmd/query.rs".to_string()],
            8,
        )
        .unwrap_err();

        assert!(err.to_string().contains("provider unavailable"));
    }

    #[test]
    fn findings_to_context_section_preserves_autoresearch_item_metadata() {
        let profile = ResearchProfile::new(
            "context packets".to_string(),
            vec!["context".to_string(), "packets".to_string()],
        );
        let source = ResearchSource::new(
            "https://example.com/context-packets".to_string(),
            "Context packet adapter notes".to_string(),
            SourceType::Article,
        );
        let entry = ResearchEntry::from_source(&source, &profile, 1.0);
        let finding = prepared_finding_from_entry(&entry);

        let section = findings_to_context_section(&[finding]).unwrap();

        assert_eq!(section.id, "autoresearch");
        assert_eq!(section.items.len(), 1);
        assert_eq!(section.items[0].id, "autoresearch-1");
        assert_eq!(section.items[0].source.kind, "autoresearch");
        assert!(section.items[0].body.contains("Freshness:"));
        assert!(section.items[0].body.contains("Provenance:"));
    }

    #[test]
    fn source_add_scan_and_search_roundtrip() {
        let store = AutoresearchStore::open_memory().unwrap();
        let source = ResearchSource::new(
            "https://example.com/ai-safety".to_string(),
            "AI Safety Weekly".to_string(),
            SourceType::Web,
        );
        store.insert_source(&source).unwrap();
        let profile = ResearchProfile::new("AI radar".to_string(), vec!["AI".to_string()]);
        store.insert_profile(&profile).unwrap();

        let run = run_scan_once(&store, Some(&profile.id)).unwrap();
        assert_eq!(run.profiles_scanned, 1);
        assert_eq!(run.sources_considered, 1);
        assert_eq!(run.entries_created, 1);
        assert_eq!(run.matches_created, 1);

        let results = store.search_entries("Safety", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].summary.as_deref(), Some("AI Safety Weekly"));
    }

    #[test]
    fn negative_keywords_exclude_scan_candidate() {
        let store = AutoresearchStore::open_memory().unwrap();
        let source = ResearchSource::new(
            "https://example.com/ai-advertising".to_string(),
            "AI Advertising".to_string(),
            SourceType::Web,
        );
        store.insert_source(&source).unwrap();
        let mut profile = ResearchProfile::new("AI radar".to_string(), vec!["AI".to_string()]);
        profile.negative_keywords = vec!["advertising".to_string()];
        store.insert_profile(&profile).unwrap();

        let run = run_scan_once(&store, Some(&profile.id)).unwrap();
        assert_eq!(run.entries_created, 0);
        assert_eq!(store.search_entries("AI", 10).unwrap().len(), 0);
    }

    #[test]
    fn scan_is_idempotent_by_source() {
        let store = AutoresearchStore::open_memory().unwrap();
        let source = ResearchSource::new(
            "https://example.com/ai".to_string(),
            "AI Note".to_string(),
            SourceType::Web,
        );
        store.insert_source(&source).unwrap();
        let profile = ResearchProfile::new("AI radar".to_string(), vec!["AI".to_string()]);
        store.insert_profile(&profile).unwrap();

        assert_eq!(
            run_scan_once(&store, Some(&profile.id))
                .unwrap()
                .entries_created,
            1
        );
        assert_eq!(
            run_scan_once(&store, Some(&profile.id))
                .unwrap()
                .entries_created,
            0
        );
    }

    #[test]
    fn same_source_can_create_task_specific_matches_for_different_profiles() {
        let store = AutoresearchStore::open_memory().unwrap();
        let source = ResearchSource::new(
            "https://example.com/layers-autoresearch-context-packets".to_string(),
            "Layers autoresearch bridges ContextPacket provenance".to_string(),
            SourceType::Web,
        );
        store.insert_source(&source).unwrap();
        let profile_a = ResearchProfile::new(
            "context packets".to_string(),
            vec!["ContextPacket".to_string(), "provenance".to_string()],
        );
        let profile_b = ResearchProfile::new(
            "autoresearch".to_string(),
            vec!["autoresearch".to_string(), "Layers".to_string()],
        );
        store.insert_profile(&profile_a).unwrap();
        store.insert_profile(&profile_b).unwrap();

        let first_run = run_scan_once(&store, Some(&profile_a.id)).unwrap();
        assert_eq!(first_run.entries_created, 1);
        assert_eq!(first_run.matches_created, 1);

        let second_run = run_scan_once(&store, Some(&profile_b.id)).unwrap();
        assert_eq!(
            second_run.matches_created, 1,
            "a source already captured for one profile must still create a task/profile-specific match for another profile"
        );
        assert_eq!(
            store
                .list_matches_for_profile(&profile_b.id, 10)
                .unwrap()
                .len(),
            1
        );
    }
}
