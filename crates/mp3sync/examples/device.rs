use std::error::Error;

use mp3sync::{Config, Syncer};

fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let syncer = Syncer::new(Config::from_env()?);

    println!("needs sync: {}", syncer.needs_sync()?);

    let plan = syncer.pending()?;
    println!(
        "pending: {} copy, {} rename, {} delete",
        plan.copy.len(),
        plan.rename.len(),
        plan.delete.len()
    );
    for step in plan.copy.iter().take(5) {
        println!("  + {}", step.to.display());
    }

    if std::env::args().any(|a| a == "--apply") {
        let report = syncer.sync()?;
        println!(
            "\napplied: {} copied, {} renamed, {} deleted, {} failed",
            report.copied,
            report.renamed,
            report.deleted,
            report.failed.len()
        );
        for failure in &report.failed {
            println!("  {} -> {}", failure.path.display(), failure.error);
        }
        println!("needs sync now: {}", syncer.needs_sync()?);
    }

    Ok(())
}
