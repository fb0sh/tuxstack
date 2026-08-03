//! `tuxstack volumes` — list volumes.

use tuxstack_docker_core::services::volumes::ListVolumesOptions;

use crate::error::CliError;
use crate::output;

use super::CommandContext;

pub async fn run(ctx: &CommandContext, filter: Option<String>) -> Result<(), CliError> {
    let options = ListVolumesOptions { search: filter };
    let volumes = ctx.services.volumes.list_volumes(&options).await?;

    if ctx.json {
        output::print_json(&volumes)?;
        return Ok(());
    }

    let mut table = output::Table::new(vec!["NAME", "DRIVER", "MOUNTPOINT", "SCOPE"]);
    for v in volumes {
        table.row(vec![
            v.name.clone(),
            v.driver.clone(),
            v.mountpoint.clone(),
            v.scope.clone(),
        ]);
    }
    table.render(&mut std::io::stdout()).map_err(CliError::Io)?;
    Ok(())
}
