use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "tuxstack")]
#[command(about = "Docker + Incus GUI for Linux desktop")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List containers
    Ps,
    /// View container logs
    Logs {
        container_id: String,
        #[arg(short, long, default_value = "50")]
        tail: usize,
    },
    /// Start a container
    Start {
        container_id: String,
    },
    /// Stop a container
    Stop {
        container_id: String,
    },
    /// Restart a container
    Restart {
        container_id: String,
    },
    /// Start the daemon
    Daemon,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Ps => {
            // TODO: connect to daemon via Unix socket, call docker.list_containers
            println!("Containers (not yet implemented)");
        }
        Commands::Logs {
            container_id,
            tail: _,
        } => {
            println!("Logs for {container_id} (not yet implemented)");
        }
        Commands::Start { container_id } => {
            println!("Starting {container_id} (not yet implemented)");
        }
        Commands::Stop { container_id } => {
            println!("Stopping {container_id} (not yet implemented)");
        }
        Commands::Restart { container_id } => {
            println!("Restarting {container_id} (not yet implemented)");
        }
        Commands::Daemon => {
            println!("Starting daemon...");
            tuxstack_daemon::Daemon::new().await?.run().await?;
        }
    }

    Ok(())
}
