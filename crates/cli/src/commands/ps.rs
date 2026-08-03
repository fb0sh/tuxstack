//! `tuxstack ps` — list containers.

use tuxstack_docker_core::services::containers::ListContainersOptions;
use tuxstack_docker_core::ContainerSummary;

use crate::error::CliError;
use crate::output;

use super::CommandContext;

pub struct PsArgs {
    pub all: bool,
    pub running: bool,
    pub filter: Option<String>,
}

pub async fn run(ctx: &CommandContext, args: &PsArgs) -> Result<(), CliError> {
    let options = ListContainersOptions {
        all: args.all,
        search: args.filter.clone(),
        state: if args.running {
            Some(tuxstack_docker_core::ContainerState::Running)
        } else {
            None
        },
        ..Default::default()
    };

    let containers = ctx.services.containers.list_containers(&options).await?;

    if ctx.json {
        output::print_json(&containers)?;
        return Ok(());
    }

    print_table(&containers);
    Ok(())
}

fn print_table(containers: &[ContainerSummary]) {
    let mut table = output::Table::new(vec![
        "CONTAINER ID", "NAME", "IMAGE", "STATE", "STATUS", "PORTS",
    ]);
    for c in containers {
        table.row(vec![
            c.short_id.clone(),
            c.name.clone(),
            c.image.clone(),
            c.state.as_str().to_string(),
            c.status.clone(),
            c.ports
                .iter()
                .map(|p| p.display())
                .collect::<Vec<_>>()
                .join(", "),
        ]);
    }
    let _ = table.render(&mut std::io::stdout());
}
