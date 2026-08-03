//! `tuxstack start <container...>` — start containers.

use crate::error::CliError;

use super::CommandContext;

pub async fn run(ctx: &CommandContext, ids: &[String]) -> Result<(), CliError> {
    for id in ids {
        tracing::info!(container = %id, "starting container");
        ctx.services.containers.start_container(id).await?;
        println!("Started {id}");
    }
    Ok(())
}
