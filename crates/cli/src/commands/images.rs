//! `tuxstack images` — list images.

use tuxstack_docker_core::ImageSummary;
use tuxstack_docker_core::services::images::ListImagesOptions;

use crate::error::CliError;
use crate::output;

use super::CommandContext;

pub async fn run(ctx: &CommandContext, filter: Option<String>) -> Result<(), CliError> {
    let options = ListImagesOptions {
        search: filter,
    };
    let images = ctx.services.images.list_images(&options).await?;

    if ctx.json {
        output::print_json(&images)?;
        return Ok(());
    }

    let mut table = output::Table::new(vec!["IMAGE ID", "TAGS", "CREATED", "SIZE"]);
    for i in images {
        table.row(vec![
            i.short_id.clone(),
            i.repository_tags.join(", "),
            i.created_at.format("%Y-%m-%d %H:%M").to_string(),
            output::size_cell(i.size_bytes),
        ]);
    }
    table
        .render(&mut std::io::stdout())
        .map_err(CliError::Io)?;
    Ok(())
}

#[allow(dead_code)]
fn _type_anchor(_: &ImageSummary) {}
