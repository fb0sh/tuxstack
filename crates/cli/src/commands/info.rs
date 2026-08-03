//! `tuxstack info` — show Docker Engine details.

use tuxstack_docker_core::models::DockerSystemInfo;

use crate::error::CliError;
use crate::output;

use super::CommandContext;

pub async fn run(ctx: &CommandContext) -> Result<(), CliError> {
    let info = ctx.services.system.system_info().await?;
    if ctx.json {
        output::print_json(&info)?;
    } else {
        print_info(&info)?;
    }
    Ok(())
}

fn print_info(info: &DockerSystemInfo) -> Result<(), CliError> {
    let mut table = output::Table::new(vec!["Key", "Value"]);
    table.row(vec!["Docker version".into(), info.version.clone()]);
    table.row(vec!["API version".into(), info.api_version.clone()]);
    table.row(vec!["Min API version".into(), info.min_api_version.clone()]);
    table.row(vec!["Server version".into(), info.server_version.clone()]);
    table.row(vec![
        "Operating system".into(),
        info.operating_system.clone(),
    ]);
    table.row(vec!["OS type".into(), info.os.clone()]);
    table.row(vec!["Architecture".into(), info.arch.clone()]);
    table.row(vec!["Kernel version".into(), info.kernel_version.clone()]);
    table.row(vec!["Docker root dir".into(), info.docker_root_dir.clone()]);
    table.row(vec!["Storage driver".into(), info.driver.clone()]);
    table.row(vec![
        "Total memory".into(),
        output::size_cell(info.total_memory),
    ]);
    table.row(vec!["CPUs".into(), info.n_cpus.to_string()]);
    table.row(vec!["Containers".into(), info.containers.to_string()]);
    table.row(vec!["Running".into(), info.containers_running.to_string()]);
    table.row(vec!["Paused".into(), info.containers_paused.to_string()]);
    table.row(vec!["Stopped".into(), info.containers_stopped.to_string()]);
    table.row(vec!["Images".into(), info.images.to_string()]);
    table.render(&mut std::io::stdout()).map_err(CliError::Io)?;
    Ok(())
}
