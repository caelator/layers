//! `layers infrastructure` — manage cloud and infrastructure credentials.
//!
//! Credentials stored in ~/.layers/infrastructure.json (gitignored).
//! Supported: SSH, Fly.io, Vercel, Cloudflare, Hetzner, Render, Railway, GitHub.
//!
//! Usage:
//!   layers infrastructure setup        # interactive wizard
//!   layers infrastructure list        # show configured providers
//!   layers infrastructure remove <p>  # remove provider
//!   layers infrastructure test       # test all connections
//!   layers infrastructure ssh add <alias> <connection> [--key path]
//!   layers infrastructure webhook setup [--cf-token X] [--github-secret Y]

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::io::{self, Write};

#[derive(Parser, Debug)]
#[command(author, version)]
pub enum InfrastructureArgs {
    /// Interactive setup wizard for infrastructure credentials.
    Setup,
    /// List all configured providers.
    List,
    /// Remove credentials for a provider.
    Remove { provider: String },
    /// Test connectivity to all configured providers.
    Test,
    /// Manage SSH host aliases.
    Ssh {
        #[command(subcommand)]
        command: SshCommands,
    },
    /// Manage GitHub webhook relay endpoint.
    Webhook {
        #[command(subcommand)]
        command: WebhookCommands,
    },
}

#[derive(Subcommand, Debug)]
pub enum SshCommands {
    Add {
        alias: String,
        connection: String,
        #[arg(long)]
        key: Option<String>,
        #[arg(long)]
        provider: Option<String>,
    },
    List,
    Remove { alias: String },
}

#[derive(Subcommand, Debug)]
pub enum WebhookCommands {
    Setup {
        #[arg(long)]
        cf_token: Option<String>,
        #[arg(long)]
        cf_account: Option<String>,
        #[arg(long)]
        github_secret: Option<String>,
    },
    Status,
    Remove,
}

pub fn handle_infrastructure(args: &InfrastructureArgs) -> Result<()> {
    match args {
        InfrastructureArgs::Setup => setup_wizard(),
        InfrastructureArgs::List => list_providers(),
        InfrastructureArgs::Remove { provider } => remove_provider(provider),
        InfrastructureArgs::Test => test_connections(),
        InfrastructureArgs::Ssh { command } => handle_ssh(command),
        InfrastructureArgs::Webhook { command } => handle_webhook(command),
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Config file
// ────────────────────────────────────────────────────────────────────────────

#[derive(Default, Serialize, Deserialize)]
struct InfraConfig {
    providers: HashMap<String, ProviderConfig>,
    ssh: HashMap<String, SshHost>,
    webhook_relay: Option<WebhookRelayConfig>,
}

#[derive(Serialize, Deserialize)]
struct ProviderConfig {
    api_token: Option<String>,
    api_secret: Option<String>,
    account_id: Option<String>,
    #[serde(flatten)]
    extra: HashMap<String, String>,
}

#[derive(Serialize, Deserialize)]
struct SshHost {
    connection: String,
    key: String,
    provider: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct WebhookRelayConfig {
    cf_account: String,
    cf_token: String,
    github_secret: String,
    local_port: String,
    worker_url: Option<String>,
}

fn infra_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join(".layers/infrastructure.json")
}

fn load_cfg() -> Result<InfraConfig> {
    let path = infra_path();
    if !path.exists() {
        return Ok(InfraConfig::default());
    }
    let content = std::fs::read_to_string(&path).context("reading infrastructure.json")?;
    serde_json::from_str(&content).context("parsing infrastructure.json")
}

fn save_cfg(cfg: &InfraConfig) -> Result<()> {
    let path = infra_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let content = serde_json::to_string_pretty(cfg).context("serializing config")?;
    std::fs::write(&path, content).context("writing infrastructure.json")?;
    Ok(())
}

// ────────────────────────────────────────────────────────────────────────────
// Prompt helper
// ────────────────────────────────────────────────────────────────────────────

fn prompt(label: &str) -> String {
    print!("{label}: ");
    io::stdout().flush().ok();
    let mut s = String::new();
    io::stdin().read_line(&mut s).ok();
    s.trim().to_string()
}

// ────────────────────────────────────────────────────────────────────────────
// Setup wizard
// ────────────────────────────────────────────────────────────────────────────

fn setup_wizard() -> Result<()> {
    println!("layers infrastructure setup — credential manager");
    println!("Credentials stored in ~/.layers/infrastructure.json (gitignored).");
    println!();

    let mut cfg = load_cfg()?;
    let mut done = false;

    while !done {
        println!("Select a provider to configure:");
        println!("  [1] SSH host");
        println!("  [2] Fly.io");
        println!("  [3] Vercel");
        println!("  [4] Cloudflare (Workers, R2, Pages)");
        println!("  [5] Hetzner Cloud");
        println!("  [6] Render");
        println!("  [7] Railway");
        println!("  [8] GitHub (token + webhook secret)");
        println!("  [9] GitHub webhook relay (Cloudflare Worker)");
        println!("  [q] Quit");
        println!();
        let input = prompt("Choice");
        match input.trim() {
            "1" => setup_ssh(&mut cfg)?,
            "2" => setup_provider(&mut cfg, "fly", "Fly.io API token", true)?,
            "3" => setup_provider(&mut cfg, "vercel", "Vercel API token", true)?,
            "4" => setup_cloudflare(&mut cfg)?,
            "5" => setup_provider(&mut cfg, "hetzner", "Hetzner Cloud API token", true)?,
            "6" => setup_provider(&mut cfg, "render", "Render API token", true)?,
            "7" => setup_provider(&mut cfg, "railway", "Railway API token", true)?,
            "8" => setup_github(&mut cfg)?,
            "9" => setup_webhook_relay(&mut cfg)?,
            "q" | "Q" => done = true,
            _ => println!("Invalid choice."),
        }
        println!();
    }

    save_cfg(&cfg)?;
    println!("Saved ~/.layers/infrastructure.json");
    Ok(())
}

fn setup_ssh(cfg: &mut InfraConfig) -> Result<()> {
    let alias = prompt("SSH alias (e.g. prod-server)");
    if alias.is_empty() {
        return Ok(());
    }
    let connection = prompt("Connection (user@host or user@host:port)");
    if connection.is_empty() {
        return Ok(());
    }
    let key = prompt("Private key path (~/.ssh/id_ed25519 if blank)");
    let key = if key.is_empty() { "~/.ssh/id_ed25519".into() } else { key };
    let provider = prompt("Provider label (or blank for 'ssh')");
    cfg.ssh.insert(alias.clone(), SshHost {
        connection,
        key,
        provider: if provider.is_empty() { None } else { Some(provider) },
    });
    println!("Added SSH host: {alias}");
    Ok(())
}

fn setup_provider(cfg: &mut InfraConfig, name: &str, label: &str, _is_api: bool) -> Result<()> {
    let token = prompt(label);
    if token.is_empty() {
        return Ok(());
    }
    cfg.providers.insert(name.into(), ProviderConfig {
        api_token: Some(token),
        api_secret: None,
        account_id: None,
        extra: HashMap::default(),
    });
    println!("Saved {name} credentials.");
    Ok(())
}

fn setup_cloudflare(cfg: &mut InfraConfig) -> Result<()> {
    let account = prompt("Cloudflare Account ID");
    let token = prompt("Cloudflare API Token");
    if token.is_empty() || account.is_empty() {
        println!("Both account and token required.");
        return Ok(());
    }
    cfg.providers.insert("cloudflare".into(), ProviderConfig {
        api_token: Some(token),
        api_secret: None,
        account_id: Some(account),
        extra: HashMap::default(),
    });
    println!("Saved Cloudflare credentials.");
    Ok(())
}

fn setup_github(cfg: &mut InfraConfig) -> Result<()> {
    let token = prompt("GitHub personal access token");
    let secret = prompt("Webhook secret (blank to skip)");
    if token.is_empty() {
        return Ok(());
    }
    let mut p = ProviderConfig {
        api_token: Some(token),
        api_secret: None,
        account_id: None,
        extra: HashMap::default(),
    };
    if !secret.is_empty() {
        p.extra.insert("webhook_secret".into(), secret);
    }
    cfg.providers.insert("github".into(), p);
    println!("Saved GitHub credentials.");
    Ok(())
}

fn setup_webhook_relay(cfg: &mut InfraConfig) -> Result<()> {
    println!();
    println!("GitHub webhooks need a public HTTPS endpoint.");
    println!("This deploys a Cloudflare Worker that relays events to your local monitor.");
    println!();
    let account = prompt("Cloudflare Account ID");
    let cf_token = prompt("Cloudflare API Token");
    let github_secret = prompt("GitHub webhook secret");
    let port = prompt("Local relay port [default: 18790]");
    let port = if port.is_empty() { "18790".into() } else { port };

    if cf_token.is_empty() || account.is_empty() {
        println!("Account and token required.");
        return Ok(());
    }

    let worker_script = include_str!("../infrastructure/webhook_relay_worker.js")
        .replace("{{GITHUB_SECRET}}", &github_secret)
        .replace("{{RELAY_SECRET}}", &rand_secret());

    let worker_path = std::env::var("HOME").unwrap_or_default()
        + "/.layers/infrastructure/webhook-relay-worker.js";
    std::fs::create_dir_all(std::path::Path::new(&worker_path).parent().unwrap())?;
    std::fs::write(&worker_path, &worker_script)?;
    println!("Worker script: {worker_path}");
    println!("Deploy: wrangler deploy --config ~/.layers/infrastructure/wrangler.toml {worker_path}");

    cfg.webhook_relay = Some(WebhookRelayConfig {
        cf_account: account,
        cf_token,
        github_secret,
        local_port: port,
        worker_url: None,
    });
    println!("Saved webhook relay config.");
    Ok(())
}

// ────────────────────────────────────────────────────────────────────────────
// SSH subcommands
// ────────────────────────────────────────────────────────────────────────────

fn handle_ssh(cmd: &SshCommands) -> Result<()> {
    let mut cfg = load_cfg()?;
    match cmd {
        SshCommands::Add { alias, connection, key, provider } => {
            let key_path = key.clone().unwrap_or_else(|| "~/.ssh/id_ed25519".into());
            cfg.ssh.insert(alias.clone(), SshHost {
                connection: connection.clone(),
                key: key_path,
                provider: provider.clone(),
            });
            save_cfg(&cfg)?;
            println!("Added SSH host: {alias} → {connection}");
        }
        SshCommands::List => {
            if cfg.ssh.is_empty() {
                println!("No SSH hosts configured.");
            } else {
                for (alias, host) in &cfg.ssh {
                    println!("  {}: {} (key: {})", alias, host.connection, host.key);
                }
            }
        }
        SshCommands::Remove { alias } => {
            if cfg.ssh.remove(alias).is_some() {
                save_cfg(&cfg)?;
                println!("Removed SSH host: {alias}");
            } else {
                println!("SSH host not found: {alias}");
            }
        }
    }
    Ok(())
}

// ────────────────────────────────────────────────────────────────────────────
// Webhook subcommands
// ────────────────────────────────────────────────────────────────────────────

fn handle_webhook(cmd: &WebhookCommands) -> Result<()> {
    let mut cfg = load_cfg()?;
    match cmd {
        WebhookCommands::Setup { cf_token, cf_account, github_secret } => {
            let account = cf_account.clone().unwrap_or_else(|| prompt("Cloudflare Account ID"));
            let token = cf_token.clone().unwrap_or_else(|| prompt("Cloudflare API Token"));
            let secret = github_secret.clone().unwrap_or_else(|| prompt("GitHub webhook secret"));
            let port = prompt("Local relay port [blank: 18790]");
            let port = if port.is_empty() { "18790".into() } else { port };

            if token.is_empty() || account.is_empty() {
                println!("Account and token required.");
                return Ok(());
            }

            let relay_secret = rand_secret();
            let worker_script = include_str!("../infrastructure/webhook_relay_worker.js")
                .replace("{{GITHUB_SECRET}}", &secret)
                .replace("{{RELAY_SECRET}}", &relay_secret);

            let worker_path = std::env::var("HOME").unwrap_or_default()
                + "/.layers/infrastructure/webhook-relay-worker.js";
            std::fs::create_dir_all(std::path::Path::new(&worker_path).parent().unwrap())?;
            std::fs::write(&worker_path, &worker_script)?;

            cfg.webhook_relay = Some(WebhookRelayConfig {
                cf_account: account,
                cf_token: token,
                github_secret: secret,
                local_port: port,
                worker_url: None,
            });
            save_cfg(&cfg)?;
            println!("Worker script: {worker_path}");
            println!("Deploy with: wrangler deploy --config ~/.layers/infrastructure/wrangler.toml {worker_path}");
            println!("Then add the Worker URL to GitHub → repo Settings → Webhooks.");
        }
        WebhookCommands::Status => {
            if let Some(relay) = &cfg.webhook_relay {
                println!("GitHub webhook relay: configured");
                println!("  Cloudflare account: {}", relay.cf_account);
                println!("  Local port: {}", relay.local_port);
                println!("  Worker URL: {}", relay.worker_url.as_ref().unwrap_or(&"(not set — deploy worker and update)".into()));
            } else {
                println!("Webhook relay: not configured.");
                println!("Run: layers infrastructure webhook setup");
            }
        }
        WebhookCommands::Remove => {
            cfg.webhook_relay = None;
            save_cfg(&cfg)?;
            println!("Webhook relay config removed.");
        }
    }
    Ok(())
}

// ────────────────────────────────────────────────────────────────────────────
// List / Remove / Test
// ────────────────────────────────────────────────────────────────────────────

fn list_providers() -> Result<()> {
    let cfg = load_cfg()?;
    let mut any = false;
    for (name, p) in &cfg.providers {
        any = true;
        let token_status = if p.api_token.is_some() || p.api_secret.is_some() {
            "✓ configured"
        } else {
            "⚠ partial"
        };
        println!("  {name}: {token_status}");
    }
    if !cfg.ssh.is_empty() {
        any = true;
        for alias in cfg.ssh.keys() {
            println!("  ssh:{alias}: ✓ configured");
        }
    }
    if cfg.webhook_relay.is_some() {
        any = true;
        println!("  github-webhook-relay: ✓ configured");
    }
    if !any {
        println!("No infrastructure configured. Run: layers infrastructure setup");
    }
    Ok(())
}

fn remove_provider(name: &str) -> Result<()> {
    let mut cfg = load_cfg()?;
    if cfg.providers.remove(name).is_some()
        || cfg.ssh.remove(name).is_some()
        || (name == "github-webhook-relay" && { cfg.webhook_relay = None; true })
    {
        save_cfg(&cfg)?;
        println!("Removed: {name}");
    } else {
        println!("Provider not found: {name}");
    }
    Ok(())
}

fn test_connections() -> Result<()> {
    let cfg = load_cfg()?;
    println!("Testing infrastructure connections...");
    for (name, p) in &cfg.providers {
        let result = match name.as_str() {
            "fly" => test_url(p, "https://api.fly.io/api/v1/me", "Authorization"),
            "vercel" => test_url(p, "https://api.vercel.com/v2/user", "Authorization"),
            "cloudflare" => {
                if let Some(account) = &p.account_id {
                    test_url(p, &format!("https://api.cloudflare.com/client/v4/accounts/{account}"), "Authorization")
                } else {
                    println!("  ✗ {name}: no account_id");
                    continue;
                }
            }
            "hetzner" => test_url(p, "https://api.hetzner.cloud/v1/servers", "Authorization"),
            "render" => test_url(p, "https://api.render.com/v1/owners", "Authorization"),
            "railway" => test_url(p, "https://backboard.railway.app/api/v1/me", "Authorization"),
            "github" => test_url(p, "https://api.github.com/user", "Authorization"),
            _ => {
                println!("  ? {name}: no test available");
                continue;
            }
        };
        match result {
            Ok(_) => println!("  ✓ {name}"),
            Err(e) => println!("  ✗ {name}: {e}"),
        }
    }
    if !cfg.ssh.is_empty() {
        println!("  ✓ ssh: {} hosts (connectivity untested)", cfg.ssh.len());
    }
    if cfg.webhook_relay.is_some() {
        println!("  ✓ github-webhook-relay: configured (test via 'webhook status')");
    }
    Ok(())
}

fn rand_secret() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    // Lower 64 bits of nanosecond timestamp — truncation is intentional for ID generation.
    #[allow(clippy::cast_possible_truncation)]
    let nanos_lower = now.as_nanos() as u64;
    let pid = u64::from(std::process::id());
    format!("{:016X}{:016X}", nanos_lower, pid.wrapping_mul(0xF00D_CAFE))
}

fn test_url(cfg: &ProviderConfig, url: &str, _auth_header: &str) -> Result<()> {
    let token = cfg.api_token.as_ref().or(cfg.api_secret.as_ref()).context("no token")?;
    let output = std::process::Command::new("curl")
        .args(["-s", "-o", "/dev/null", "-w", "%{http_code}", "-H", &format!("Authorization: Bearer {token}"), url])
        .output()
        .context("curl failed")?;
    let code = String::from_utf8_lossy(&output.stdout);
    if code.contains("200") {
        Ok(())
    } else {
        Err(anyhow::anyhow!("HTTP {code}"))
    }
}
