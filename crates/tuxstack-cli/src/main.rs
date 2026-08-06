use anyhow::{Context, Result, bail};
use futures_util::StreamExt;
use std::env;
use std::os::fd::RawFd;
use std::process::ExitCode;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::signal::unix::{SignalKind, signal};
use tokio_util::sync::CancellationToken;
use tuxstack_client::{
    Client, ClientConfig, ContainerTerminalError, ContainerTerminalOptions,
    ContainerTerminalOutput, DaemonServices,
};
use tuxstack_protocol::{DockerResourceRef, MountAction, Request, Response, ShellSelection};

#[derive(Debug, Clone, PartialEq, Eq)]
struct ShellCommand {
    container: String,
    shell: ShellSelection,
    user: Option<String>,
    workdir: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CliCommand {
    Status,
    Mount(MountAction),
    Path { kind: String, id: String },
    ContainerShell(ShellCommand),
    TerminalTest,
    Help,
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(code) => code,
        Err(error) => {
            eprintln!("tuxstack-cli: {error:#}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<ExitCode> {
    let command = parse_args(env::args().skip(1))?;
    match command {
        CliCommand::Help => {
            usage();
            Ok(ExitCode::SUCCESS)
        }
        CliCommand::TerminalTest => {
            println!("TuxStack terminal test");
            println!();
            println!("Terminal launch is working.");
            println!("You can close this window.");
            Ok(ExitCode::SUCCESS)
        }
        CliCommand::ContainerShell(command) => run_container_shell(command).await,
        CliCommand::Status | CliCommand::Mount(_) | CliCommand::Path { .. } => {
            let config = ClientConfig::from_env(env!("CARGO_PKG_VERSION"))
                .context("locate tuxstackd control socket")?;
            let client = Client::connect(config)
                .await
                .context("connect to tuxstackd")?;
            run_control_command(client, command).await
        }
    }
}

async fn run_control_command(client: Client, command: CliCommand) -> Result<ExitCode> {
    match command {
        CliCommand::Status => match client.request(Request::GetDaemonStatus).await? {
            Response::DaemonStatus(status) => {
                println!("daemon: {:?}", status.lifecycle);
                println!("docker: {:?}", status.docker);
                println!("filesystem: {:?}", status.mount.state);
                if let Some(path) = status.mount.mount_point {
                    println!("mount: {}", path.display());
                }
            }
            response => unexpected(response)?,
        },
        CliCommand::Mount(action) => match client.request(Request::SetMountState(action)).await? {
            Response::MountStatus(status) => println!("{:?}", status.state),
            response => unexpected(response)?,
        },
        CliCommand::Path { kind, id } => {
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
                response => unexpected(response)?,
            }
        }
        _ => unreachable!("control command was checked before connecting"),
    }
    Ok(ExitCode::SUCCESS)
}

async fn run_container_shell(command: ShellCommand) -> Result<ExitCode> {
    require_interactive_terminal()?;
    let (rows, cols) = terminal_size(libc::STDOUT_FILENO).context("read terminal size")?;
    let config = ClientConfig::from_env(env!("CARGO_PKG_VERSION"))
        .context("locate tuxstackd control socket")?;
    let client = Client::connect(config)
        .await
        .context("connect to tuxstackd")?;

    let ShellCommand {
        container,
        shell,
        user,
        workdir,
    } = command;
    let services = DaemonServices::new(std::sync::Arc::new(client));
    let cancellation = CancellationToken::new();
    let session = services
        .container_terminal
        .connect(
            &container,
            ContainerTerminalOptions {
                shell,
                user,
                workdir,
                rows,
                cols,
            },
            cancellation.clone(),
        )
        .await
        .map_err(|error| anyhow::anyhow!(terminal_message(error)))
        .context("open container terminal")?;

    // The client service starts subscriptions with its safe default size. Set
    // the actual local size before enabling raw mode, so the remote TTY sees
    // the dimensions users see from the first prompt onward.
    let _ = session.resize(rows, cols).await;
    let mut raw = match RawTerminalGuard::new(libc::STDIN_FILENO) {
        Ok(raw) => raw,
        Err(error) => {
            session.close().await;
            return Err(error).context("enable raw terminal mode");
        }
    };
    let terminal_result = drive_terminal(&session, rows, cols).await;
    let restore_result = raw.restore();
    session.close().await;

    restore_result.context("restore terminal mode")?;
    let status_code = terminal_result?;
    Ok(to_exit_code(status_code))
}

async fn drive_terminal(
    session: &tuxstack_client::ContainerTerminalSession,
    initial_rows: u16,
    initial_cols: u16,
) -> Result<i64> {
    let mut output = session
        .take_output()
        .await
        .map_err(|error| anyhow::anyhow!(terminal_message(error)))?;
    let mut stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let mut window_change = signal(SignalKind::window_change()).context("listen for SIGWINCH")?;
    let mut input = [0_u8; 8192];

    // Send again after raw mode is active. This also makes the initial size
    // path explicit for daemons which delay creating the exec until Running.
    let _ = session.resize(initial_rows, initial_cols).await;

    loop {
        tokio::select! {
            result = stdin.read(&mut input) => {
                let count = result.context("read terminal input")?;
                if count == 0 {
                    break;
                }
                session.write_input(input[..count].to_vec())
                    .await
                    .map_err(|error| anyhow::anyhow!(terminal_message(error)))?;
            }
            item = output.next() => {
                match item {
                    Some(Ok(ContainerTerminalOutput::Console(bytes))) => {
                        stdout.write_all(&bytes).await.context("write terminal output")?;
                        stdout.flush().await.context("flush terminal output")?;
                    }
                    Some(Err(error)) => bail!(terminal_message(error)),
                    None => {
                        let status = session.inspect().await
                            .map_err(|error| anyhow::anyhow!(terminal_message(error)))?;
                        return Ok(status.exit_code.unwrap_or(1));
                    }
                }
            }
            _ = window_change.recv() => {
                // A local size query or remote resize failure must not tear
                // down an otherwise usable interactive session; a later
                // SIGWINCH can retry it.
                if let Ok((rows, cols)) = terminal_size(libc::STDOUT_FILENO) {
                    let _ = session.resize(rows, cols).await;
                }
            }
        }
    }
    Ok(0)
}

fn terminal_message(error: ContainerTerminalError) -> String {
    error.to_string()
}

fn to_exit_code(code: i64) -> ExitCode {
    let code = if code < 0 { 1 } else { code };
    ExitCode::from(code.clamp(0, u8::MAX as i64) as u8)
}

fn require_interactive_terminal() -> Result<()> {
    if !is_terminal(libc::STDIN_FILENO) || !is_terminal(libc::STDOUT_FILENO) {
        bail!("An interactive terminal is required.");
    }
    Ok(())
}

fn is_terminal(fd: RawFd) -> bool {
    // SAFETY: `isatty` only reads the supplied process-local file descriptor.
    unsafe { libc::isatty(fd) == 1 }
}

fn terminal_size(fd: RawFd) -> Result<(u16, u16)> {
    let mut size = libc::winsize {
        ws_row: 0,
        ws_col: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: `size` is a valid writable winsize and ioctl does not retain it.
    let result = unsafe { libc::ioctl(fd, libc::TIOCGWINSZ, &mut size) };
    if result == -1 {
        return Err(std::io::Error::last_os_error().into());
    }
    // Some pseudo terminals report zero during startup. Use the conventional
    // fallback rather than sending an invalid dimension to the daemon.
    Ok((size.ws_row.max(1), size.ws_col.max(1)))
}

struct RawTerminalGuard {
    fd: RawFd,
    original: libc::termios,
    active: bool,
}

impl RawTerminalGuard {
    fn new(fd: RawFd) -> Result<Self> {
        let mut original = std::mem::MaybeUninit::<libc::termios>::uninit();
        // SAFETY: `original` is valid storage for tcgetattr to initialize.
        if unsafe { libc::tcgetattr(fd, original.as_mut_ptr()) } == -1 {
            return Err(std::io::Error::last_os_error().into());
        }
        // SAFETY: tcgetattr initialized the value above; cfmakeraw only edits
        // this local termios structure.
        let original = unsafe { original.assume_init() };
        let mut raw = original;
        // SAFETY: raw is a valid termios value obtained from this terminal.
        unsafe { libc::cfmakeraw(&mut raw) };
        // SAFETY: raw is valid and the fd is owned by this process.
        if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &raw) } == -1 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(Self {
            fd,
            original,
            active: true,
        })
    }

    fn restore(&mut self) -> Result<()> {
        if !self.active {
            return Ok(());
        }
        // SAFETY: original was read from this terminal and remains local.
        let result = unsafe { libc::tcsetattr(self.fd, libc::TCSANOW, &self.original) };
        if result == -1 {
            return Err(std::io::Error::last_os_error().into());
        }
        self.active = false;
        Ok(())
    }
}

impl Drop for RawTerminalGuard {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<CliCommand> {
    let mut args = args.into_iter();
    let Some(command) = args.next() else {
        return Ok(CliCommand::Status);
    };
    match command.as_str() {
        "help" | "--help" | "-h" => Ok(CliCommand::Help),
        "status" => {
            no_more(args)?;
            Ok(CliCommand::Status)
        }
        "mount" | "unmount" | "remount" => {
            no_more(args)?;
            let action = match command.as_str() {
                "mount" => MountAction::Mount,
                "unmount" => MountAction::Unmount,
                _ => MountAction::Remount,
            };
            Ok(CliCommand::Mount(action))
        }
        "path" => {
            let kind = args.next().context("usage: tuxstack-cli path KIND ID")?;
            let id = args.next().context("usage: tuxstack-cli path KIND ID")?;
            no_more(args)?;
            Ok(CliCommand::Path { kind, id })
        }
        "container" => parse_container_command(args),
        "terminal" => parse_terminal_command(args),
        _ => bail!("unknown command {command:?}; run tuxstack-cli help"),
    }
}

fn parse_container_command(mut args: impl Iterator<Item = String>) -> Result<CliCommand> {
    let Some(subcommand) = args.next() else {
        bail!("usage: tuxstack-cli container shell <container> [OPTIONS]");
    };
    match subcommand.as_str() {
        "shell" => parse_shell_command(args),
        "help" | "--help" | "-h" => Ok(CliCommand::Help),
        _ => bail!("unknown container command {subcommand:?}; run tuxstack-cli help"),
    }
}

fn parse_terminal_command(mut args: impl Iterator<Item = String>) -> Result<CliCommand> {
    let Some(subcommand) = args.next() else {
        bail!("usage: tuxstack-cli terminal test");
    };
    match subcommand.as_str() {
        "test" => {
            if matches!(args.next().as_deref(), Some("--help" | "-h")) {
                return Ok(CliCommand::Help);
            }
            no_more(args)?;
            Ok(CliCommand::TerminalTest)
        }
        "help" | "--help" | "-h" => Ok(CliCommand::Help),
        _ => bail!("unknown terminal command {subcommand:?}; run tuxstack-cli help"),
    }
}

fn parse_shell_command(mut args: impl Iterator<Item = String>) -> Result<CliCommand> {
    let Some(container) = args.next() else {
        bail!("usage: tuxstack-cli container shell <container> [OPTIONS]");
    };
    if matches!(container.as_str(), "--help" | "-h") {
        return Ok(CliCommand::Help);
    }
    let mut command = ShellCommand {
        container,
        shell: ShellSelection::Auto,
        user: None,
        workdir: None,
    };
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--help" | "-h" => return Ok(CliCommand::Help),
            "--shell" => {
                command.shell = parse_shell_selection(
                    args.next()
                        .context("--shell requires auto or an absolute path")?,
                )?;
            }
            "--user" => command.user = Some(non_empty_option("--user", args.next())?),
            "--workdir" => command.workdir = Some(non_empty_option("--workdir", args.next())?),
            option if option.starts_with("--shell=") => {
                command.shell = parse_shell_selection(option[8..].to_owned())?;
            }
            option if option.starts_with("--user=") => {
                command.user = Some(non_empty_option("--user", Some(option[7..].to_owned()))?);
            }
            option if option.starts_with("--workdir=") => {
                command.workdir = Some(non_empty_option(
                    "--workdir",
                    Some(option[10..].to_owned()),
                )?);
            }
            _ => bail!("unexpected argument {argument:?} in container shell command"),
        }
    }
    Ok(CliCommand::ContainerShell(command))
}

fn parse_shell_selection(value: String) -> Result<ShellSelection> {
    if value == "auto" {
        return Ok(ShellSelection::Auto);
    }
    if value.starts_with('/') && !value.as_bytes().contains(&0) {
        return Ok(ShellSelection::ExactPath(value));
    }
    bail!("--shell must be auto or an absolute path")
}

fn non_empty_option(name: &str, value: Option<String>) -> Result<String> {
    let value = value.with_context(|| format!("{name} requires a value"))?;
    if value.is_empty() || value.as_bytes().contains(&0) {
        bail!("{name} must not be empty or contain NUL")
    }
    Ok(value)
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
        "tuxstack-cli [status]\n\
         tuxstack-cli mount|unmount|remount\n\
         tuxstack-cli path container|image|volume ID\n\
         tuxstack-cli container shell <container> [--shell auto|/absolute/path] [--user USER] [--workdir PATH]\n\
         tuxstack-cli terminal test\n\n\
         container shell connects through tuxstackd.\n\
         The container must be running and not paused.\n\
         An interactive terminal is required for container shell."
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extra_arguments_are_rejected() {
        assert!(no_more(["unexpected".to_owned()].into_iter()).is_err());
        assert!(no_more(Vec::<String>::new().into_iter()).is_ok());
    }

    #[test]
    fn parses_shell_options_without_shell_string_concatenation() {
        let command = parse_args(
            [
                "container",
                "shell",
                "container id",
                "--shell",
                "/bin/zsh",
                "--user=alice",
                "--workdir",
                "/workspace",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .unwrap();
        assert_eq!(
            command,
            CliCommand::ContainerShell(ShellCommand {
                container: "container id".into(),
                shell: ShellSelection::ExactPath("/bin/zsh".into()),
                user: Some("alice".into()),
                workdir: Some("/workspace".into()),
            })
        );
    }

    #[test]
    fn shell_requires_auto_or_absolute_path() {
        assert!(parse_shell_selection("bash".into()).is_err());
        assert!(parse_shell_selection("relative/path".into()).is_err());
        assert_eq!(
            parse_shell_selection("auto".into()).unwrap(),
            ShellSelection::Auto
        );
    }

    #[test]
    fn nested_help_does_not_require_a_daemon() {
        assert_eq!(
            parse_args(["container", "shell", "--help"].map(str::to_owned)).unwrap(),
            CliCommand::Help
        );
        assert_eq!(
            parse_args(["terminal", "test", "--help"].map(str::to_owned)).unwrap(),
            CliCommand::Help
        );
    }

    #[test]
    fn raw_mode_clears_canonical_echo_and_signals() {
        let mut termios = unsafe { std::mem::zeroed::<libc::termios>() };
        termios.c_lflag = libc::ICANON | libc::ECHO | libc::ISIG;
        // SAFETY: termios is initialized storage and cfmakeraw only mutates it.
        unsafe { libc::cfmakeraw(&mut termios) };
        assert_eq!(
            termios.c_lflag & (libc::ICANON | libc::ECHO | libc::ISIG),
            0
        );
    }

    #[test]
    fn raw_guard_restores_a_pseudoterminal() {
        let mut master = -1;
        let mut slave = -1;
        // SAFETY: openpty initializes these two output descriptors.
        let result = unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        assert_eq!(result, 0);

        let mut before = unsafe { std::mem::zeroed::<libc::termios>() };
        // SAFETY: slave is the valid descriptor returned by openpty.
        assert_eq!(unsafe { libc::tcgetattr(slave, &mut before) }, 0);
        {
            let _guard = RawTerminalGuard::new(slave).unwrap();
            let mut raw = unsafe { std::mem::zeroed::<libc::termios>() };
            // SAFETY: slave is still open and raw is writable storage.
            assert_eq!(unsafe { libc::tcgetattr(slave, &mut raw) }, 0);
            assert_eq!(raw.c_lflag & (libc::ICANON | libc::ECHO | libc::ISIG), 0);
        }
        let mut after = unsafe { std::mem::zeroed::<libc::termios>() };
        // SAFETY: slave remains open until the end of this test.
        assert_eq!(unsafe { libc::tcgetattr(slave, &mut after) }, 0);
        assert_eq!(after.c_lflag, before.c_lflag);
        // SAFETY: descriptors were returned by openpty and are closed once.
        unsafe {
            libc::close(master);
            libc::close(slave);
        }
    }

    #[test]
    fn non_tty_file_descriptors_are_not_terminals() {
        assert!(!is_terminal(-1));
    }
}
