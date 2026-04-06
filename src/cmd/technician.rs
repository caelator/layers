//! `layers technician` — self-healing plugin integration monitor.

use anyhow::Result;
use clap::Parser;

use crate::technician::{format_cycle_report, run_technician_cycle};

#[derive(Parser)]
pub enum TechnicianArgs {
    /// Run one technician monitoring cycle.
    Run {
        /// Apply repairs (default is dry-run: diagnose only).
        #[arg(long)]
        apply: bool,
    },
    /// Print the current technician state and recent escalations.
    Status,
}

pub fn handle_technician(args: &TechnicianArgs) -> Result<()> {
    match args {
        TechnicianArgs::Run { apply } => {
            let report = run_technician_cycle(!apply)?;
            println!("{}", format_cycle_report(&report));
            if !apply {
                println!("(dry-run: no repairs applied; use --apply to enable repairs)");
            }
            if !report.escalations.is_empty() {
                println!(
                    "\n{} escalation(s) written to ~/.layers/technician-escalations.jsonl",
                    report.escalations.len()
                );
            }
        }
        TechnicianArgs::Status => {
            let state = crate::technician::data::TechnicianState::load();
            println!("Technician State");
            println!("  Cycle count:     {}", state.cycle_count);
            println!("  Last cycle:      {}", state.last_cycle_ts);
            println!("  UC available:    {}", state.uc_available);
            println!("  GitNexus avail:  {}", state.gitnexus_available);
            println!(
                "  Council failed:  {} (7d)",
                state.council_runs_failed_7d
            );
            println!(
                "  Pending esc.:    {}",
                state.pending_escalations
            );

            // Print recent escalations
            let esc_path = crate::technician::data::escalations_path();
            if esc_path.exists() {
                println!("\nRecent Escalations (last 24h):");
                let content = std::fs::read_to_string(&esc_path)?;
                let cutoff = chrono::Utc::now() - chrono::Duration::hours(24);
                let mut count = 0;
                for line in content.lines().rev() {
                    if line.trim().is_empty() {
                        continue;
                    }
                    if let Ok(record) =
                        serde_json::from_str::<crate::technician::data::EscalationRecord>(line)
                    {
                        if let Ok(ts) =
                            chrono::DateTime::parse_from_rfc3339(&record.ts)
                        {
                            if ts.with_timezone(&chrono::Utc) >= cutoff {
                                println!(
                                    "  [{}] {} — {}",
                                    record.ts, record.diagnosis, record.escalation_reason
                                );
                                count += 1;
                            }
                        }
                    }
                    if count >= 10 {
                        break;
                    }
                }
                if count == 0 {
                    println!("  (none)");
                }
            } else {
                println!("\nNo escalations on record.");
            }
        }
    }
    Ok(())
}
