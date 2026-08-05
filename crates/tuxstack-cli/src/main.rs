use std::env;
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use tuxstack_client::{Client, ClientConfig};
use tuxstack_protocol::{DockerResourceRef, MountAction, Request, Response};

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("tuxstackctl: {error:#}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<()> {
    let mut args = env::args().skip(1);
    let command = args.next().unwrap_or_else(|| "status".to_owned());
    let config = ClientConfig::from_env(env!("CARGO_PKG_VERSION"))
        .context("locate tuxstackd control socket")?;
    let client = Client::connect(config)
        .await
        .context("connect to tuxstackd")?;
    match command.as_str() {
        "status" => {
            no_more(args)?;
            match client.request(Request::GetDaemonStatus).await? {
                Response::DaemonStatus(status) => {
                    println!("daemon: {:?}", status.lifecycle);
                    println!("docker: {:?}", status.docker);
                    println!("filesystem: {:?}", status.mount.state);
                    if let Some(path) = status.mount.mount_point {
                        println!("mount: {}", path.display());
                    }
                }
                response => unexpected(response)?,
            }
        }
        "mount" | "unmount" | "remount" => {
            no_more(args)?;
            let action = match command.as_str() {
                "mount" => MountAction::Mount,
                "unmount" => MountAction::Unmount,
                _ => MountAction::Remount,
            };
            match client.request(Request::SetMountState(action)).await? {
                Response::MountStatus(status) => println!("{:?}", status.state),
                response => unexpected(response)?,
            }
        }
        "path" => {
            let kind = args.next().context("usage: tuxstackctl path KIND ID")?;
            let id = args.next().context("usage: tuxstackctl path KIND ID")?;
            no_more(args)?;
            let resource = match kind.as_str() {
                "container" => DockerResourceRef::Container { container_id: id },
                "image" => DockerResourceRef::Image { image_id: id },
                "volume" => DockerResourceRef::Volume { volume_name: id },
                _ => bail!("KIND must be container, image, or volume"),
            };
            match client
                .request(Request::GetResourceFusePath(resource))
                .await?
            {
                Response::ResourceFusePath(path) => println!("{}", path.path.display()),
                Response::Error(error) => bail!("{}", error.message),
                response => unexpected(response)?,
            }
        }
        "help" | "--help" | "-h" => usage(),
        _ => bail!("unknown command {command:?}; run tuxstackctl help"),
    }
    Ok(())
}

fn no_more(mut args: impl Iterator<Item = String>) -> Result<()> {
    if args.next().is_some() {
        bail!("unexpected extra arguments");
    }
    Ok(())
}

fn unexpected(response: Response) -> Result<()> {
    match response {
        Response::Error(error) => bail!("{}", error.message),
        _ => bail!("unexpected daemon response"),
    }
}

fn usage() {
    println!(
        "tuxstackctl [status]\n\
         tuxstackctl mount|unmount|remount\n\
         tuxstackctl path container|image|volume ID"
    );
}

#[cfg(test)]
mod tests {
    use super::no_more;

    #[test]
    fn extra_arguments_are_rejected() {
        assert!(no_more(["unexpected".to_owned()].into_iter()).is_err());
        assert!(no_more(Vec::<String>::new().into_iter()).is_ok());
    }
}
