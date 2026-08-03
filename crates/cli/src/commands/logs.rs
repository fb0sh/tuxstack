//! `tuxstack logs <container>` — show container logs.

use std::io::Write;

use futures_util::StreamExt;
use tuxstack_docker_core::{ContainerLogsOptions, LogLine};

use crate::error::CliError;

use super::CommandContext;

pub struct LogsArgs {
    pub container: String,
    pub follow: bool,
    pub tail: Option<usize>,
    pub timestamps: bool,
    pub since: Option<i64>,
    pub until: Option<i64>,
}

pub async fn run(ctx: &CommandContext, args: &LogsArgs) -> Result<(), CliError> {
    let since = args
        .since
        .map(|s| chrono::DateTime::from_timestamp(s, 0));
    let until = args
        .until
        .map(|u| chrono::DateTime::from_timestamp(u, 0));

    let options = ContainerLogsOptions {
        stdout: true,
        stderr: true,
        timestamps: args.timestamps,
        follow: args.follow,
        tail: args.tail,
        since: since.flatten(),
        until: until.flatten(),
    };

    if args.follow {
        let cancel = tokio_util::sync::CancellationToken::new();
        let mut stream =
            ctx.services
                .containers
                .watch_logs(&args.container, &options, cancel.clone());
        while let Some(line) = stream.next().await {
            match line {
                Ok(line) => print_line(&line, args.timestamps),
                Err(e) => return Err(CliError::Docker(e)),
            }
        }
        Ok(())
    } else {
        let lines = ctx
            .services
            .containers
            .container_logs(&args.container, &options)
            .await?;
        for line in lines {
            print_line(&line, args.timestamps);
        }
        Ok(())
    }
}

fn print_line(line: &LogLine, timestamps: bool) {
    let mut out = std::io::stdout();
    if timestamps {
        if let Some(ts) = line.timestamp {
            let _ = write!(out, "{} ", ts.to_rfc3339());
        }
    }
    let _ = writeln!(out, "{}", line.message);
}
