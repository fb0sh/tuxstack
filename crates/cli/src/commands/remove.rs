//! `tuxstack rm <container...>` — remove containers.

use tuxstack_docker_core::RemoveContainerOptions;

use crate::error::CliError;

use super::CommandContext;

pub struct RmArgs {
    pub ids: Vec<String>,
    pub force: bool,
    pub volumes: bool,
}

pub async fn run(ctx: &CommandContext, args: &RmArgs) -> Result<(), CliError> {
    let options = RemoveContainerOptions {
        force: args.force,
        remove_volumes: args.volumes,
        remove_links: false,
    };
    for id in &args.ids {
        tracing::info!(container = %id, "removing container");
        ctx.services
            .containers
            .remove_container(id, &options)
            .await?;
        println!("Removed {id}");
    }
    Ok(())
}
