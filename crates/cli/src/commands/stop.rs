//! `tuxstack stop <container...>` — stop containers.

use tuxstack_docker_core::StopContainerOptions;

use crate::error::CliError;

use super::CommandContext;

pub struct StopArgs {
    pub ids: Vec<String>,
    pub timeout: Option<i64>,
}

pub async fn run(ctx: &CommandContext, args: &StopArgs) -> Result<(), CliError> {
    let options = StopContainerOptions {
        timeout_seconds: args.timeout,
    };
    for id in &args.ids {
        tracing::info!(container = %id, "stopping container");
        ctx.services
            .containers
            .stop_container(id, Some(&options))
            .await?;
        println!("Stopped {id}");
    }
    Ok(())
}
