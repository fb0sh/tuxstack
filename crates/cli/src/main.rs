//! tuxstack — command-line Docker management tool.
//!
//! Uses the same `tuxstack-docker-core` library as the GUI.

mod commands;
mod error;
mod output;

use clap::{Args, Parser, Subcommand};

use crate::error::{CliError, exit};

#[derive(Parser)]
#[command(
    name = "tuxstack",
    about = "Native Docker management for KDE Plasma (CLI)",
    version
)]
struct Cli {
    #[command(flatten)]
    global: GlobalArgs,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Args, Clone)]
struct GlobalArgs {
    /// Docker host (e.g. unix:///var/run/docker.sock, tcp://127.0.0.1:2375)
    #[arg(long, global = true)]
    host: Option<String>,

    /// Per-operation timeout in seconds
    #[arg(long, global = true)]
    timeout: Option<u64>,

    /// Output machine-readable JSON
    #[arg(long, global = true)]
    json: bool,

    /// Enable debug logging (RUST_LOG=debug)
    #[arg(long, global = true)]
    debug: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Show Docker Engine information
    Info,
    /// List containers
    Ps {
        /// Include stopped containers
        #[arg(short, long)]
        all: bool,
        /// Only running containers
        #[arg(long)]
        running: bool,
        /// Filter by name or image substring
        #[arg(long)]
        filter: Option<String>,
    },
    /// Inspect a container
    Inspect {
        /// Container ID or name
        container: String,
    },
    /// Show container logs
    Logs {
        /// Container ID or name
        container: String,
        /// Follow log output
        #[arg(short, long)]
        follow: bool,
        /// Number of lines to show (default: all)
        #[arg(long)]
        tail: Option<usize>,
        /// Show timestamps
        #[arg(long)]
        timestamps: bool,
        /// Only show logs since this unix timestamp
        #[arg(long)]
        since: Option<i64>,
        /// Only show logs until this unix timestamp
        #[arg(long)]
        until: Option<i64>,
    },
    /// Start containers
    Start {
        /// Container IDs or names
        #[arg(required = true)]
        containers: Vec<String>,
    },
    /// Stop containers
    Stop {
        /// Container IDs or names
        #[arg(required = true)]
        containers: Vec<String>,
        /// Grace period in seconds before SIGKILL
        #[arg(short, long)]
        timeout: Option<i64>,
    },
    /// Restart containers
    Restart {
        /// Container IDs or names
        #[arg(required = true)]
        containers: Vec<String>,
    },
    /// Pause containers
    Pause {
        /// Container IDs or names
        #[arg(required = true)]
        containers: Vec<String>,
    },
    /// Unpause containers
    Unpause {
        /// Container IDs or names
        #[arg(required = true)]
        containers: Vec<String>,
    },
    /// Remove containers
    Rm {
        /// Container IDs or names
        #[arg(required = true)]
        containers: Vec<String>,
        /// Force removal of running containers
        #[arg(short, long)]
        force: bool,
        /// Remove anonymous volumes
        #[arg(long)]
        volumes: bool,
    },
    /// List images
    Images {
        /// Filter by tag or ID substring
        #[arg(long)]
        filter: Option<String>,
    },
    /// List networks
    Networks {
        /// Filter by name or ID substring
        #[arg(long)]
        filter: Option<String>,
    },
    /// List volumes
    Volumes {
        /// Filter by name substring
        #[arg(long)]
        filter: Option<String>,
    },
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let cli = Cli::parse();

    let level = if cli.global.debug {
        "debug".to_string()
    } else {
        std::env::var("TUXSTACK_LOG")
            .or_else(|_| std::env::var("RUST_LOG"))
            .unwrap_or_else(|_| "info".to_string())
    };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_new(&level).unwrap_or_else(|_| "info".into()),
        )
        .with_target(false)
        .init();

    match run(cli).await {
        Ok(()) => std::process::ExitCode::from(exit::OK),
        Err(err) => {
            eprintln!("error: {err}");
            let code = err.exit_code();
            if code == exit::DOCKER_UNAVAILABLE {
                eprintln!("hint: is the Docker Engine running and the socket accessible?");
            } else if code == exit::PERMISSION_DENIED {
                eprintln!(
                    "hint: your user needs access to the Docker socket (e.g. the docker group)"
                );
            }
            std::process::ExitCode::from(code)
        }
    }
}

async fn run(cli: Cli) -> Result<(), CliError> {
    let ctx =
        commands::CommandContext::build(cli.global.host, cli.global.timeout, cli.global.json)?;

    match cli.command {
        Commands::Info => commands::info::run(&ctx).await,
        Commands::Ps {
            all,
            running,
            filter,
        } => {
            commands::ps::run(
                &ctx,
                &commands::ps::PsArgs {
                    all,
                    running,
                    filter,
                },
            )
            .await
        }
        Commands::Inspect { container } => commands::inspect::run(&ctx, &container).await,
        Commands::Logs {
            container,
            follow,
            tail,
            timestamps,
            since,
            until,
        } => {
            commands::logs::run(
                &ctx,
                &commands::logs::LogsArgs {
                    container,
                    follow,
                    tail,
                    timestamps,
                    since,
                    until,
                },
            )
            .await
        }
        Commands::Start { containers } => commands::start::run(&ctx, &containers).await,
        Commands::Stop {
            containers,
            timeout,
        } => {
            commands::stop::run(
                &ctx,
                &commands::stop::StopArgs {
                    ids: containers,
                    timeout,
                },
            )
            .await
        }
        Commands::Restart { containers } => commands::restart::run(&ctx, &containers).await,
        Commands::Pause { containers } => {
            for id in &containers {
                ctx.services.containers.pause_container(id).await?;
                println!("Paused {id}");
            }
            Ok(())
        }
        Commands::Unpause { containers } => {
            for id in &containers {
                ctx.services.containers.unpause_container(id).await?;
                println!("Unpaused {id}");
            }
            Ok(())
        }
        Commands::Rm {
            containers,
            force,
            volumes,
        } => {
            commands::remove::run(
                &ctx,
                &commands::remove::RmArgs {
                    ids: containers,
                    force,
                    volumes,
                },
            )
            .await
        }
        Commands::Images { filter } => commands::images::run(&ctx, filter).await,
        Commands::Networks { filter } => commands::networks::run(&ctx, filter).await,
        Commands::Volumes { filter } => commands::volumes::run(&ctx, filter).await,
    }
}
