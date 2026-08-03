//! `tuxstack networks` — list networks.

use tuxstack_docker_core::services::networks::ListNetworksOptions;

use crate::error::CliError;
use crate::output;

use super::CommandContext;

pub async fn run(ctx: &CommandContext, filter: Option<String>) -> Result<(), CliError> {
    let options = ListNetworksOptions {
        search: filter,
    };
    let networks = ctx.services.networks.list_networks(&options).await?;

    if ctx.json {
        output::print_json(&networks)?;
        return Ok(());
    }

    let mut table = output::Table::new(vec!["NETWORK ID", "NAME", "DRIVER", "SCOPE"]);
    for n in networks {
        table.row(vec![
            n.id.chars().take(12).collect(),
            n.name.clone(),
            n.driver.clone(),
            n.scope.clone(),
        ]);
    }
    table
        .render(&mut std::io::stdout())
        .map_err(CliError::Io)?;
    Ok(())
}
