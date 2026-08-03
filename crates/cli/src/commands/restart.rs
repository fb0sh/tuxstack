//! `tuxstack restart <container...>` — restart containers.

use crate::error::CliError;

use super::CommandContext;

pub async fn run(ctx: &CommandContext, ids: &[String]) -> Result<(), CliError> {
    for id in ids {
        tracing::info!(container = %id, "restarting container");
        ctx.services.containers.restart_container(id).await?;
        println!("Restarted {id}");
    }
    Ok(())
}
