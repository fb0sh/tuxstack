//! GUI-side system-terminal integration.
//!
//! This module deliberately contains no daemon or protocol types.  It detects
//! local terminal applications, persists only a stable terminal ID, and builds
//! argv vectors for launching the already-installed `tuxstack-cli` without a
//! shell.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum TerminalId {
    #[default]
    Auto,
    Konsole,
    Ghostty,
    Kitty,
    Foot,
    WezTerm,
    Alacritty,
    GnomeConsole,
    GnomeTerminal,
    XfceTerminal,
    MateTerminal,
    LxTerminal,
    XTerm,
}

impl TerminalId {
    pub const ALL: [Self; 13] = [
        Self::Auto,
        Self::Konsole,
        Self::Ghostty,
        Self::Kitty,
        Self::Foot,
        Self::WezTerm,
        Self::Alacritty,
        Self::GnomeConsole,
        Self::GnomeTerminal,
        Self::XfceTerminal,
        Self::MateTerminal,
        Self::LxTerminal,
        Self::XTerm,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Konsole => "konsole",
            Self::Ghostty => "ghostty",
            Self::Kitty => "kitty",
            Self::Foot => "foot",
            Self::WezTerm => "wezterm",
            Self::Alacritty => "alacritty",
            Self::GnomeConsole => "gnome-console",
            Self::GnomeTerminal => "gnome-terminal",
            Self::XfceTerminal => "xfce-terminal",
            Self::MateTerminal => "mate-terminal",
            Self::LxTerminal => "lxterminal",
            Self::XTerm => "xterm",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|id| id.as_str() == value)
    }
}

impl fmt::Display for TerminalId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalAdapterKind {
    XdgTerminalExec,
    Konsole,
    Ghostty,
    Kitty,
    Foot,
    WezTerm,
    Alacritty,
    GnomeConsole,
    GnomeTerminal,
    XfceTerminal,
    MateTerminal,
    LxTerminal,
    XTerm,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DetectedTerminal {
    pub id: TerminalId,
    pub display_name: String,
    pub executable: PathBuf,
    pub adapter: TerminalAdapterKind,
    pub desktop_preferred: bool,
    pub supports_title: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalCommand {
    pub program: PathBuf,
    pub args: Vec<OsString>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LaunchCommand {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub working_directory: Option<PathBuf>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TerminalLaunchOptions {
    pub title: Option<String>,
    pub working_directory: Option<PathBuf>,
}

#[derive(Clone, Debug, Default)]
pub struct TerminalDetector;

impl TerminalDetector {
    pub fn detect(&self) -> Vec<DetectedTerminal> {
        self.detect_with(
            std::env::var_os("PATH"),
            [
                std::env::var("XDG_CURRENT_DESKTOP").ok(),
                std::env::var("XDG_SESSION_DESKTOP").ok(),
                std::env::var("DESKTOP_SESSION").ok(),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>(),
        )
    }

    /// Detect terminals from explicit environment values.  Keeping this
    /// separate makes the security-sensitive PATH rules unit-testable without
    /// modifying the process environment.
    pub fn detect_with<I, S>(&self, path: Option<OsString>, desktops: I) -> Vec<DetectedTerminal>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let desktop = desktops
            .into_iter()
            .map(|value| value.as_ref().to_ascii_lowercase())
            .collect::<Vec<_>>();
        let candidates = candidate_order(&desktop);
        let mut terminals = Vec::new();
        let mut seen = Vec::<PathBuf>::new();

        for candidate in candidates {
            let Some(executable) = find_executable(path.as_deref(), candidate.binary) else {
                continue;
            };
            if seen.iter().any(|path| path == &executable) {
                continue;
            }
            seen.push(executable.clone());
            terminals.push(DetectedTerminal {
                id: candidate.id,
                display_name: candidate.display_name.to_owned(),
                executable,
                adapter: candidate.adapter,
                desktop_preferred: candidate.desktop_preferred(&desktop),
                supports_title: candidate.supports_title,
            });
        }
        if !terminals.is_empty()
            && !terminals
                .iter()
                .any(|terminal| terminal.id == TerminalId::Auto)
        {
            let mut automatic = terminals[0].clone();
            automatic.id = TerminalId::Auto;
            automatic.display_name = "System Default".to_owned();
            automatic.desktop_preferred = true;
            terminals.insert(0, automatic);
        }
        terminals
    }
}

#[derive(Clone, Copy)]
struct Candidate {
    id: TerminalId,
    binary: &'static str,
    display_name: &'static str,
    adapter: TerminalAdapterKind,
    supports_title: bool,
    preferred: &'static [&'static str],
}

impl Candidate {
    fn desktop_preferred(self, desktops: &[String]) -> bool {
        self.preferred
            .iter()
            .any(|desktop| desktops.iter().any(|current| current.contains(desktop)))
    }
}

const KDE: &[&str] = &["kde", "plasma"];
const GNOME: &[&str] = &["gnome"];
const WLROOTS: &[&str] = &["sway", "wlroots", "river", "hyprland"];

fn candidate_order(desktops: &[String]) -> Vec<Candidate> {
    // xdg-terminal-exec is always first: it delegates to the desktop's own
    // default rather than guessing one.  The remaining order is stable.
    let generic = [
        Candidate {
            id: TerminalId::Konsole,
            binary: "konsole",
            display_name: "Konsole",
            adapter: TerminalAdapterKind::Konsole,
            supports_title: true,
            preferred: KDE,
        },
        Candidate {
            id: TerminalId::Ghostty,
            binary: "ghostty",
            display_name: "Ghostty",
            adapter: TerminalAdapterKind::Ghostty,
            supports_title: true,
            preferred: WLROOTS,
        },
        Candidate {
            id: TerminalId::Kitty,
            binary: "kitty",
            display_name: "Kitty",
            adapter: TerminalAdapterKind::Kitty,
            supports_title: true,
            preferred: WLROOTS,
        },
        Candidate {
            id: TerminalId::Foot,
            binary: "foot",
            display_name: "Foot",
            adapter: TerminalAdapterKind::Foot,
            supports_title: true,
            preferred: WLROOTS,
        },
        Candidate {
            id: TerminalId::WezTerm,
            binary: "wezterm",
            display_name: "WezTerm",
            adapter: TerminalAdapterKind::WezTerm,
            supports_title: false,
            preferred: &[],
        },
        Candidate {
            id: TerminalId::Alacritty,
            binary: "alacritty",
            display_name: "Alacritty",
            adapter: TerminalAdapterKind::Alacritty,
            supports_title: true,
            preferred: &[],
        },
        Candidate {
            id: TerminalId::GnomeConsole,
            binary: "kgx",
            display_name: "GNOME Console",
            adapter: TerminalAdapterKind::GnomeConsole,
            supports_title: false,
            preferred: GNOME,
        },
        Candidate {
            id: TerminalId::GnomeTerminal,
            binary: "gnome-terminal",
            display_name: "GNOME Terminal",
            adapter: TerminalAdapterKind::GnomeTerminal,
            supports_title: true,
            preferred: GNOME,
        },
        Candidate {
            id: TerminalId::XfceTerminal,
            binary: "xfce4-terminal",
            display_name: "Xfce Terminal",
            adapter: TerminalAdapterKind::XfceTerminal,
            supports_title: true,
            preferred: &[],
        },
        Candidate {
            id: TerminalId::MateTerminal,
            binary: "mate-terminal",
            display_name: "MATE Terminal",
            adapter: TerminalAdapterKind::MateTerminal,
            supports_title: true,
            preferred: &[],
        },
        Candidate {
            id: TerminalId::LxTerminal,
            binary: "lxterminal",
            display_name: "LXTerminal",
            adapter: TerminalAdapterKind::LxTerminal,
            supports_title: true,
            preferred: &[],
        },
        Candidate {
            id: TerminalId::XTerm,
            binary: "xterm",
            display_name: "XTerm",
            adapter: TerminalAdapterKind::XTerm,
            supports_title: true,
            preferred: &[],
        },
    ];

    let mut ordered = Vec::with_capacity(generic.len() + 1);
    // Auto is represented by xdg-terminal-exec only when that executable is
    // present. It is not a fake row and therefore cannot be launched stale.
    ordered.push(Candidate {
        id: TerminalId::Auto,
        binary: "xdg-terminal-exec",
        display_name: "System Default",
        adapter: TerminalAdapterKind::XdgTerminalExec,
        supports_title: false,
        preferred: &[],
    });
    for candidate in generic {
        if candidate.desktop_preferred(desktops) {
            ordered.push(candidate);
        }
    }
    for candidate in generic {
        if !candidate.desktop_preferred(desktops) {
            ordered.push(candidate);
        }
    }
    ordered
}

/// Find a candidate by walking PATH directly.  No shell, `which`, or
/// `command -v` is involved.
pub fn find_executable(path: Option<&OsStr>, name: &str) -> Option<PathBuf> {
    let path = path?;
    for directory in std::env::split_paths(path) {
        let candidate = directory.join(name);
        let Ok(canonical) = fs::canonicalize(&candidate) else {
            continue;
        };
        if is_executable_file(&canonical) {
            return Some(canonical);
        }
    }
    None
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

pub trait TerminalAdapter {
    fn build_launch_command(
        &self,
        terminal: &DetectedTerminal,
        command: &ExternalCommand,
        options: &TerminalLaunchOptions,
    ) -> Result<LaunchCommand, TerminalLaunchError>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SystemTerminalAdapter;

impl TerminalAdapter for SystemTerminalAdapter {
    fn build_launch_command(
        &self,
        terminal: &DetectedTerminal,
        command: &ExternalCommand,
        options: &TerminalLaunchOptions,
    ) -> Result<LaunchCommand, TerminalLaunchError> {
        if terminal.executable.as_os_str().is_empty() || !is_executable_file(&terminal.executable) {
            return Err(TerminalLaunchError::TerminalExecutableInvalid);
        }
        let mut args = Vec::new();
        if terminal.supports_title {
            if let Some(title) = options.title.as_deref() {
                append_title(terminal.adapter, title, &mut args);
            }
        }
        match terminal.adapter {
            TerminalAdapterKind::XdgTerminalExec => {}
            TerminalAdapterKind::Konsole
            | TerminalAdapterKind::Ghostty
            | TerminalAdapterKind::Foot
            | TerminalAdapterKind::Alacritty
            | TerminalAdapterKind::XfceTerminal
            | TerminalAdapterKind::MateTerminal
            | TerminalAdapterKind::LxTerminal
            | TerminalAdapterKind::XTerm => args.push(OsString::from("-e")),
            TerminalAdapterKind::Kitty => {}
            TerminalAdapterKind::WezTerm => {
                args.extend([OsString::from("start"), OsString::from("--")]);
            }
            TerminalAdapterKind::GnomeConsole | TerminalAdapterKind::GnomeTerminal => {
                args.push(OsString::from("--"));
            }
        }
        args.push(command.program.clone().into_os_string());
        args.extend(command.args.iter().cloned());
        Ok(LaunchCommand {
            program: terminal.executable.clone(),
            args,
            working_directory: options.working_directory.clone(),
        })
    }
}

fn append_title(adapter: TerminalAdapterKind, title: &str, args: &mut Vec<OsString>) {
    let (flag, value) = match adapter {
        TerminalAdapterKind::XTerm => ("-T", title),
        TerminalAdapterKind::Konsole
        | TerminalAdapterKind::Ghostty
        | TerminalAdapterKind::Kitty
        | TerminalAdapterKind::Foot
        | TerminalAdapterKind::Alacritty
        | TerminalAdapterKind::GnomeTerminal
        | TerminalAdapterKind::XfceTerminal
        | TerminalAdapterKind::MateTerminal
        | TerminalAdapterKind::LxTerminal => ("--title", title),
        _ => return,
    };
    args.push(OsString::from(flag));
    args.push(OsString::from(value));
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TerminalLaunchError {
    NoTerminalDetected,
    SelectedTerminalUnavailable,
    TerminalExecutableInvalid,
    TuxStackCliNotFound,
    LaunchFailed(String),
    SettingsReadFailed(String),
    SettingsWriteFailed(String),
}

impl fmt::Display for TerminalLaunchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoTerminalDetected => {
                formatter.write_str("No supported terminal application was found.")
            }
            Self::SelectedTerminalUnavailable => {
                formatter.write_str("The selected terminal is unavailable.")
            }
            Self::TerminalExecutableInvalid => {
                formatter.write_str("The terminal executable is invalid.")
            }
            Self::TuxStackCliNotFound => formatter.write_str("tuxstack-cli could not be found."),
            Self::LaunchFailed(message) => write!(
                formatter,
                "The terminal application could not be started: {message}"
            ),
            Self::SettingsReadFailed(message) => {
                write!(formatter, "Could not read terminal settings: {message}")
            }
            Self::SettingsWriteFailed(message) => {
                write!(formatter, "Could not save terminal settings: {message}")
            }
        }
    }
}

impl std::error::Error for TerminalLaunchError {}

#[derive(Clone, Debug)]
pub struct TerminalConfigStore {
    path: PathBuf,
}

impl TerminalConfigStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn default_path() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
        Some(base.join("tuxstack").join("config.toml"))
    }

    pub fn read_preference(&self) -> Result<TerminalId, TerminalLaunchError> {
        let contents = match fs::read_to_string(&self.path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(TerminalId::Auto),
            Err(error) => return Err(TerminalLaunchError::SettingsReadFailed(error.to_string())),
        };
        Ok(parse_preference(&contents).unwrap_or(TerminalId::Auto))
    }

    pub fn write_preference(&self, id: TerminalId) -> Result<(), TerminalLaunchError> {
        let existing = fs::read_to_string(&self.path).unwrap_or_default();
        let contents = update_preference(&existing, id);
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)
            .map_err(|error| TerminalLaunchError::SettingsWriteFailed(error.to_string()))?;
        let temporary = parent.join(format!(
            ".{}.{}.tmp",
            self.path.file_name().unwrap_or_default().to_string_lossy(),
            std::process::id()
        ));
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                file.set_permissions(fs::Permissions::from_mode(0o600))?;
            }
            file.write_all(contents.as_bytes())?;
            file.sync_all()?;
            fs::rename(&temporary, &self.path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result.map_err(|error| TerminalLaunchError::SettingsWriteFailed(error.to_string()))
    }
}

fn parse_preference(contents: &str) -> Option<TerminalId> {
    let mut in_terminal = false;
    for raw_line in contents.lines() {
        let line = raw_line.split('#').next().unwrap_or_default().trim();
        if line.starts_with('[') {
            in_terminal = line == "[terminal]";
            continue;
        }
        if !in_terminal {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() != "application" {
            continue;
        }
        let value = value.trim();
        let value = value.strip_prefix('"')?.strip_suffix('"')?;
        return TerminalId::parse(value);
    }
    None
}

fn update_preference(contents: &str, id: TerminalId) -> String {
    let replacement = format!("application = \"{}\"", id.as_str());
    let lines = contents.lines().collect::<Vec<_>>();
    if let Some(section) = lines.iter().position(|line| line.trim() == "[terminal]") {
        let end = lines[section + 1..]
            .iter()
            .position(|line| line.trim_start().starts_with('['))
            .map(|offset| section + 1 + offset)
            .unwrap_or(lines.len());
        if let Some(application) =
            (section + 1..end).find(|index| lines[*index].trim_start().starts_with("application"))
        {
            let mut output = lines
                .iter()
                .map(|line| (*line).to_owned())
                .collect::<Vec<_>>();
            output[application] = replacement;
            return output.join("\n") + "\n";
        }
        let mut output = lines[..section + 1].join("\n");
        output.push('\n');
        output.push_str(&replacement);
        if end < lines.len() {
            output.push('\n');
            output.push_str(&lines[end..].join("\n"));
        }
        return output + "\n";
    }
    let mut output = contents.trim_end().to_owned();
    if !output.is_empty() {
        output.push_str("\n\n");
    }
    output.push_str("[terminal]\n");
    output.push_str(&replacement);
    output.push('\n');
    output
}

#[derive(Clone, Debug, Default)]
pub struct TuxStackCliResolver;

impl TuxStackCliResolver {
    pub fn resolve(&self) -> Option<PathBuf> {
        self.resolve_from(
            std::env::current_exe().ok(),
            std::env::var_os("TUXSTACK_CLI_PATH"),
            std::env::var_os("PATH"),
        )
    }

    pub fn resolve_from(
        &self,
        current_exe: Option<PathBuf>,
        override_path: Option<OsString>,
        path: Option<OsString>,
    ) -> Option<PathBuf> {
        if let Some(path) = override_path {
            let candidate = PathBuf::from(path);
            if is_executable_file(&candidate) {
                return fs::canonicalize(candidate).ok();
            }
        }
        let sibling = current_exe
            .as_deref()
            .and_then(Path::parent)
            .map(|dir| dir.join("tuxstack-cli"));
        if let Some(candidate) = sibling.filter(|candidate| is_executable_file(candidate)) {
            return fs::canonicalize(candidate).ok();
        }
        if let Some(exe) = current_exe.as_deref() {
            if let Some(prefix) = exe.parent().and_then(Path::parent) {
                let candidate = prefix.join("bin").join("tuxstack-cli");
                if is_executable_file(&candidate) {
                    return fs::canonicalize(candidate).ok();
                }
            }
        }
        find_executable(path.as_deref(), "tuxstack-cli")
    }
}

#[derive(Clone, Debug)]
pub struct LaunchContainerShellRequest {
    pub container_id: String,
    pub container_name: String,
}

#[derive(Clone, Debug)]
pub struct SystemTerminalLauncher {
    pub detector: TerminalDetector,
    pub settings: TerminalConfigStore,
    pub cli_resolver: TuxStackCliResolver,
}

impl SystemTerminalLauncher {
    pub fn launch_container_shell(
        &self,
        request: LaunchContainerShellRequest,
    ) -> Result<(), TerminalLaunchError> {
        self.launch_external(
            ExternalCommand {
                program: self.cli_path()?,
                args: vec![
                    "container".into(),
                    "shell".into(),
                    request.container_id.clone().into(),
                ],
            },
            TerminalLaunchOptions {
                title: Some(format!("TuxStack — {}", request.container_name)),
                working_directory: None,
            },
        )
    }

    pub fn launch_test_terminal(&self, terminal_id: TerminalId) -> Result<(), TerminalLaunchError> {
        let terminal = self.choose_terminal(terminal_id)?;
        let command = ExternalCommand {
            program: self.cli_path()?,
            args: vec!["terminal".into(), "test".into()],
        };
        let launch = SystemTerminalAdapter.build_launch_command(
            &terminal,
            &command,
            &TerminalLaunchOptions::default(),
        )?;
        spawn_detached(launch)
    }

    fn launch_external(
        &self,
        command: ExternalCommand,
        options: TerminalLaunchOptions,
    ) -> Result<(), TerminalLaunchError> {
        let selected = self.settings.read_preference().unwrap_or(TerminalId::Auto);
        let terminal = self.choose_terminal(selected)?;
        let launch = SystemTerminalAdapter.build_launch_command(&terminal, &command, &options)?;
        spawn_detached(launch)
    }

    fn cli_path(&self) -> Result<PathBuf, TerminalLaunchError> {
        self.cli_resolver
            .resolve()
            .ok_or(TerminalLaunchError::TuxStackCliNotFound)
    }

    fn choose_terminal(
        &self,
        requested: TerminalId,
    ) -> Result<DetectedTerminal, TerminalLaunchError> {
        let terminals = self.detector.detect();
        if terminals.is_empty() {
            return Err(TerminalLaunchError::NoTerminalDetected);
        }
        if requested != TerminalId::Auto {
            return terminals
                .into_iter()
                .find(|terminal| terminal.id == requested)
                .ok_or(TerminalLaunchError::SelectedTerminalUnavailable);
        }
        terminals
            .into_iter()
            .next()
            .ok_or(TerminalLaunchError::NoTerminalDetected)
    }
}

fn spawn_detached(launch: LaunchCommand) -> Result<(), TerminalLaunchError> {
    let mut command = Command::new(&launch.program);
    command
        .args(&launch.args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(directory) = launch.working_directory {
        command.current_dir(directory);
    }
    command
        .spawn()
        .map(|_| ())
        .map_err(|error| TerminalLaunchError::LaunchFailed(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    fn executable(dir: &Path, name: &str, mode: u32) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, b"test").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
        path
    }

    #[test]
    fn detection_walks_path_without_shell_and_deduplicates() {
        let temp = TempDir::new().unwrap();
        let first = temp.path().join("one");
        let second = temp.path().join("two");
        fs::create_dir_all(&first).unwrap();
        fs::create_dir_all(&second).unwrap();
        executable(&first, "konsole", 0o755);
        executable(&second, "kitty", 0o755);
        let path =
            std::env::join_paths([first.as_path(), first.as_path(), second.as_path()]).unwrap();
        let result = TerminalDetector.detect_with(Some(path), ["KDE"]);
        assert_eq!(
            result.iter().map(|item| item.id).collect::<Vec<_>>(),
            [TerminalId::Auto, TerminalId::Konsole, TerminalId::Kitty]
        );
        assert!(result[0].desktop_preferred);
    }

    #[test]
    fn invalid_and_non_executable_path_entries_are_ignored() {
        let temp = TempDir::new().unwrap();
        executable(temp.path(), "konsole", 0o644);
        assert!(find_executable(Some(temp.path().as_os_str()), "konsole").is_none());
        assert!(find_executable(Some(temp.path().as_os_str()), "missing").is_none());
        assert!(find_executable(None, "konsole").is_none());
    }

    fn terminal(id: TerminalId, adapter: TerminalAdapterKind) -> DetectedTerminal {
        let temp = TempDir::new().unwrap();
        let executable = executable(temp.path(), id.as_str(), 0o755);
        // The directory is intentionally leaked for the duration of this test
        // helper so the adapter's executable remains valid.
        std::mem::forget(temp);
        DetectedTerminal {
            id,
            display_name: id.to_string(),
            executable,
            adapter,
            desktop_preferred: false,
            supports_title: false,
        }
    }

    #[test]
    fn adapters_keep_program_and_arguments_separate() {
        let terminal = terminal(TerminalId::WezTerm, TerminalAdapterKind::WezTerm);
        let command = ExternalCommand {
            program: PathBuf::from("/tmp/path with spaces/tuxstack-cli"),
            args: vec!["container".into(), "shell".into(), "id with spaces".into()],
        };
        let launch = SystemTerminalAdapter
            .build_launch_command(&terminal, &command, &TerminalLaunchOptions::default())
            .unwrap();
        assert_eq!(launch.program, terminal.executable);
        assert_eq!(
            launch.args,
            [
                "start",
                "--",
                "/tmp/path with spaces/tuxstack-cli",
                "container",
                "shell",
                "id with spaces"
            ]
            .map(OsString::from)
        );
    }

    #[test]
    fn adapter_title_is_optional_and_does_not_enter_unsupported_commands() {
        let mut terminal = terminal(TerminalId::Kitty, TerminalAdapterKind::Kitty);
        terminal.supports_title = false;
        let command = ExternalCommand {
            program: PathBuf::from("cli"),
            args: vec!["terminal".into(), "test".into()],
        };
        let launch = SystemTerminalAdapter
            .build_launch_command(
                &terminal,
                &command,
                &TerminalLaunchOptions {
                    title: Some("unicode — title".into()),
                    working_directory: None,
                },
            )
            .unwrap();
        assert_eq!(launch.args, ["cli", "terminal", "test"].map(OsString::from));
    }

    #[test]
    fn config_is_atomic_shape_and_preserves_other_sections() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("config.toml");
        fs::write(&path, "[other]\nvalue = 1\n").unwrap();
        let store = TerminalConfigStore::new(path.clone());
        store.write_preference(TerminalId::Ghostty).unwrap();
        assert_eq!(store.read_preference().unwrap(), TerminalId::Ghostty);
        let contents = fs::read_to_string(path).unwrap();
        assert!(contents.contains("[other]"));
        assert!(contents.contains("application = \"ghostty\""));
    }

    #[test]
    fn corrupt_or_unavailable_preference_falls_back_to_auto() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("config.toml");
        fs::write(&path, "[terminal]\napplication = \"removed-terminal\"\n").unwrap();
        assert_eq!(
            TerminalConfigStore::new(path).read_preference().unwrap(),
            TerminalId::Auto
        );
    }

    #[test]
    fn resolver_prefers_override_then_sibling_then_path() {
        let temp = TempDir::new().unwrap();
        let sibling = temp.path().join("tuxstack-cli");
        executable(temp.path(), "tuxstack-cli", 0o755);
        let path_dir = temp.path().join("path");
        fs::create_dir(&path_dir).unwrap();
        let path_cli = executable(&path_dir, "tuxstack-cli", 0o755);
        let resolver = TuxStackCliResolver;
        let resolved = resolver
            .resolve_from(
                Some(temp.path().join("tuxstack")),
                None,
                Some(path_dir.clone().into_os_string()),
            )
            .unwrap();
        assert_eq!(resolved, fs::canonicalize(sibling).unwrap());
        assert_eq!(
            resolver.resolve_from(None, None, Some(path_dir.into_os_string())),
            Some(fs::canonicalize(path_cli).unwrap())
        );
    }
}
