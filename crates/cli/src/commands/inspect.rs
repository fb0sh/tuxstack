//! `tuxstack inspect <container>` — show container details.

use crate::error::CliError;
use crate::output;

use super::CommandContext;

pub async fn run(ctx: &CommandContext, id: &str) -> Result<(), CliError> {
    let detail = ctx.services.containers.inspect_container(id).await?;

    if ctx.json {
        output::print_json(&detail)?;
        return Ok(());
    }

    let summary = &detail.summary;
    let mut table = output::Table::new(vec!["Key", "Value"]);
    table.row(vec!["ID".into(), summary.id.clone()]);
    table.row(vec!["Name".into(), summary.name.clone()]);
    table.row(vec!["Image".into(), summary.image.clone()]);
    table.row(vec!["State".into(), summary.state.as_str().to_string()]);
    table.row(vec!["Status".into(), summary.status.clone()]);
    table.row(vec!["Created".into(), summary.created_at.to_rfc3339()]);
    table.row(vec![
        "Command".into(),
        detail.command.join(" "),
    ]);
    table.row(vec![
        "Entrypoint".into(),
        detail.entrypoint.join(" "),
    ]);
    table.row(vec![
        "Ports".into(),
        summary
            .ports
            .iter()
            .map(|p| p.display())
            .collect::<Vec<_>>()
            .join(", "),
    ]);
    table.row(vec![
        "Mounts".into(),
        detail
            .mounts
            .iter()
            .map(|m| {
                let src = m.source.clone().unwrap_or_default();
                format!("{src}:{}", m.destination)
            })
            .collect::<Vec<_>>()
            .join(", "),
    ]);
    table.row(vec![
        "Networks".into(),
        detail
            .networks
            .iter()
            .map(|n| n.network_name.clone())
            .collect::<Vec<_>>()
            .join(", "),
    ]);
    table.row(vec![
        "Restart policy".into(),
        match detail.restart_policy.maximum_retry_count {
            Some(n) => format!("{} (max {n})", detail.restart_policy.name),
            None => detail.restart_policy.name.clone(),
        },
    ]);
    if let Some(health) = &detail.health {
        table.row(vec!["Health".into(), health.status.clone()]);
    }
    table
        .render(&mut std::io::stdout())
        .map_err(CliError::Io)?;
    Ok(())
}
