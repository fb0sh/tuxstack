use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

#[derive(Clone, Debug)]
pub struct DaemonPaths {
    pub runtime_dir: PathBuf,
    pub socket_path: PathBuf,
    pub spool_dir: PathBuf,
    pub mount_point: PathBuf,
    pub cache_dir: PathBuf,
}

impl DaemonPaths {
    pub fn from_env() -> Result<Self> {
        let runtime_root = absolute_env_path("XDG_RUNTIME_DIR")?;
        let home = absolute_env_path("HOME")?;
        let cache_root = env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".cache"));
        if !cache_root.is_absolute() {
            bail!("XDG_CACHE_HOME must be absolute");
        }
        let runtime_dir = runtime_root.join("tuxstack");
        Ok(Self {
            socket_path: runtime_dir.join("control.sock"),
            spool_dir: runtime_dir.join("spool"),
            runtime_dir,
            mount_point: home.join("TuxStack/docker"),
            cache_dir: cache_root.join("tuxstack"),
        })
    }

    pub fn prepare(&self) -> Result<()> {
        secure_directory(&self.runtime_dir)?;
        secure_directory(&self.spool_dir)?;
        secure_directory(&self.cache_dir)?;
        let parent = self
            .mount_point
            .parent()
            .context("mount point has no parent")?;
        secure_directory(parent)?;
        // The mount directory is owned by the FUSE filesystem while the
        // daemon is running. chmod(2) on that read-only mount returns EROFS;
        // verify it only when it is an ordinary directory. A stale mount is
        // recovered by DaemonState::start before the next mount is created.
        if !is_mountpoint(&self.mount_point) {
            secure_directory(&self.mount_point)?;
        }
        Ok(())
    }
}

fn absolute_env_path(name: &str) -> Result<PathBuf> {
    let value = env::var_os(name).with_context(|| format!("{name} is not set"))?;
    let path = PathBuf::from(value);
    if !path.is_absolute() {
        bail!("{name} must be absolute");
    }
    Ok(path)
}

fn is_mountpoint(path: &Path) -> bool {
    std::process::Command::new("mountpoint")
        .arg("-q")
        .arg(path)
        .status()
        .is_ok_and(|status| status.success())
}

fn secure_directory(path: &Path) -> Result<()> {
    fs::create_dir_all(path).with_context(|| format!("create {}", path.display()))?;
    let metadata =
        fs::symlink_metadata(path).with_context(|| format!("inspect {}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("{} must be a real directory", path.display());
    }
    if metadata.uid() != unsafe { libc::geteuid() } {
        bail!("{} is not owned by the current user", path.display());
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .with_context(|| format!("secure {}", path.display()))?;
    Ok(())
}

use std::os::unix::fs::MetadataExt;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secure_directory_rejects_symlink() {
        let root = tempfile::tempdir().unwrap();
        let real = root.path().join("real");
        fs::create_dir(&real).unwrap();
        let link = root.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(secure_directory(&link).is_err());
    }
}
