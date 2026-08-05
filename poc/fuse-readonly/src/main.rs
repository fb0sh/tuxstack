#[cfg(not(feature = "abi-7-31"))]
compile_error!("build this PoC with --features abi-7-31");

use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, UNIX_EPOCH};

use fuser::{
    AccessFlags, BsdFileFlags, Config, CopyFileRangeFlags, Errno, FileAttr, FileHandle, FileType,
    Filesystem, FopenFlags, Generation, INodeNo, LockOwner, MountOption, OpenAccMode, OpenFlags,
    RenameFlags, ReplyAttr, ReplyCreate, ReplyData, ReplyDirectory, ReplyEmpty, ReplyEntry,
    ReplyOpen, ReplyStatfs, ReplyWrite, Request, TimeOrNow, WriteFlags,
};

const ROOT_INO: INodeNo = INodeNo::ROOT;
const HELLO_INO: INodeNo = INodeNo(2);
const SLOW_INO: INodeNo = INodeNo(3);
const HELLO_NAME: &str = "hello.txt";
const SLOW_NAME: &str = ".tuxstack-slow-read";
const HELLO: &[u8] = b"tuxstack-fuse-poc\n";
const SLOW: &[u8] = b"delayed read complete\n";
const SLOW_DELAY: Duration = Duration::from_secs(3);
const TTL: Duration = Duration::from_secs(1);
const FILE_HANDLE: FileHandle = FileHandle(1);

static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);

#[derive(Debug)]
struct ReadOnlyFs {
    uid: u32,
    gid: u32,
}

impl ReadOnlyFs {
    fn attr(&self, ino: INodeNo) -> Option<FileAttr> {
        let (size, blocks, kind, perm, nlink) = match ino {
            ROOT_INO => (0, 0, FileType::Directory, 0o555, 2),
            HELLO_INO => (
                HELLO.len() as u64,
                HELLO.len().div_ceil(512) as u64,
                FileType::RegularFile,
                0o444,
                1,
            ),
            SLOW_INO => (
                SLOW.len() as u64,
                SLOW.len().div_ceil(512) as u64,
                FileType::RegularFile,
                0o444,
                1,
            ),
            _ => return None,
        };

        Some(FileAttr {
            ino,
            size,
            blocks,
            atime: UNIX_EPOCH,
            mtime: UNIX_EPOCH,
            ctime: UNIX_EPOCH,
            crtime: UNIX_EPOCH,
            kind,
            perm,
            nlink,
            uid: self.uid,
            gid: self.gid,
            rdev: 0,
            blksize: 4096,
            flags: 0,
        })
    }
}

impl Filesystem for ReadOnlyFs {
    fn lookup(&self, _req: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEntry) {
        let ino = if parent == ROOT_INO && name == OsStr::new(HELLO_NAME) {
            Some(HELLO_INO)
        } else if parent == ROOT_INO && name == OsStr::new(SLOW_NAME) {
            Some(SLOW_INO)
        } else {
            None
        };

        match ino.and_then(|ino| self.attr(ino)) {
            Some(attr) => reply.entry(&TTL, &attr, Generation(0)),
            None => reply.error(Errno::ENOENT),
        }
    }

    fn getattr(&self, _req: &Request, ino: INodeNo, _fh: Option<FileHandle>, reply: ReplyAttr) {
        match self.attr(ino) {
            Some(attr) => reply.attr(&TTL, &attr),
            None => reply.error(Errno::ENOENT),
        }
    }

    fn readdir(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        offset: u64,
        mut reply: ReplyDirectory,
    ) {
        if ino != ROOT_INO {
            reply.error(Errno::ENOTDIR);
            return;
        }

        // The delayed probe is intentionally lookup-only so normal listings contain
        // only the required hello.txt entry. See --help or tests/smoke.sh.
        let entries = [
            (ROOT_INO, FileType::Directory, "."),
            (ROOT_INO, FileType::Directory, ".."),
            (HELLO_INO, FileType::RegularFile, HELLO_NAME),
        ];
        let start = usize::try_from(offset).unwrap_or(entries.len());
        for (index, (entry_ino, kind, name)) in entries.iter().enumerate().skip(start) {
            if reply.add(*entry_ino, (index + 1) as u64, *kind, name) {
                break;
            }
        }
        reply.ok();
    }

    fn open(&self, _req: &Request, ino: INodeNo, flags: OpenFlags, reply: ReplyOpen) {
        if ino != HELLO_INO && ino != SLOW_INO {
            reply.error(if ino == ROOT_INO {
                Errno::EISDIR
            } else {
                Errno::ENOENT
            });
        } else if !is_read_only_open(flags.0) {
            reply.error(Errno::EROFS);
        } else {
            // Direct I/O makes every delayed-probe invocation reach read(), avoiding
            // page-cache hits that would make the concurrency check nondeterministic.
            reply.opened(FILE_HANDLE, FopenFlags::FOPEN_DIRECT_IO);
        }
    }

    fn read(
        &self,
        _req: &Request,
        ino: INodeNo,
        fh: FileHandle,
        offset: u64,
        size: u32,
        _flags: OpenFlags,
        _lock_owner: Option<LockOwner>,
        reply: ReplyData,
    ) {
        if fh != FILE_HANDLE {
            reply.error(Errno::EBADF);
            return;
        }

        let content = match ino {
            HELLO_INO => HELLO,
            SLOW_INO => {
                thread::sleep(SLOW_DELAY);
                SLOW
            }
            ROOT_INO => {
                reply.error(Errno::EISDIR);
                return;
            }
            _ => {
                reply.error(Errno::ENOENT);
                return;
            }
        };
        reply.data(read_at(content, offset, size));
    }

    fn release(
        &self,
        _req: &Request,
        ino: INodeNo,
        fh: FileHandle,
        _flags: OpenFlags,
        _lock_owner: Option<LockOwner>,
        _flush: bool,
        reply: ReplyEmpty,
    ) {
        if (ino == HELLO_INO || ino == SLOW_INO) && fh == FILE_HANDLE {
            reply.ok();
        } else {
            reply.error(Errno::EBADF);
        }
    }

    fn statfs(&self, _req: &Request, _ino: INodeNo, reply: ReplyStatfs) {
        // This is a virtual namespace, not Docker or host storage capacity. Report
        // one allocated virtual block and no writable/free capacity.
        reply.statfs(1, 0, 0, 2, 0, 4096, 255, 4096);
    }

    fn access(&self, _req: &Request, ino: INodeNo, mask: AccessFlags, reply: ReplyEmpty) {
        if self.attr(ino).is_none() {
            reply.error(Errno::ENOENT);
        } else if mask.contains(AccessFlags::W_OK) {
            reply.error(Errno::EROFS);
        } else if ino != ROOT_INO && mask.contains(AccessFlags::X_OK) {
            reply.error(Errno::EACCES);
        } else {
            reply.ok();
        }
    }

    fn setattr(
        &self,
        _req: &Request,
        _ino: INodeNo,
        _mode: Option<u32>,
        _uid: Option<u32>,
        _gid: Option<u32>,
        _size: Option<u64>,
        _atime: Option<TimeOrNow>,
        _mtime: Option<TimeOrNow>,
        _ctime: Option<std::time::SystemTime>,
        _fh: Option<FileHandle>,
        _crtime: Option<std::time::SystemTime>,
        _chgtime: Option<std::time::SystemTime>,
        _bkuptime: Option<std::time::SystemTime>,
        _flags: Option<BsdFileFlags>,
        reply: ReplyAttr,
    ) {
        reply.error(Errno::EROFS);
    }

    fn mknod(
        &self,
        _req: &Request,
        _parent: INodeNo,
        _name: &OsStr,
        _mode: u32,
        _umask: u32,
        _rdev: u32,
        reply: ReplyEntry,
    ) {
        reply.error(Errno::EROFS);
    }

    fn mkdir(
        &self,
        _req: &Request,
        _parent: INodeNo,
        _name: &OsStr,
        _mode: u32,
        _umask: u32,
        reply: ReplyEntry,
    ) {
        reply.error(Errno::EROFS);
    }

    fn unlink(&self, _req: &Request, _parent: INodeNo, _name: &OsStr, reply: ReplyEmpty) {
        reply.error(Errno::EROFS);
    }

    fn rmdir(&self, _req: &Request, _parent: INodeNo, _name: &OsStr, reply: ReplyEmpty) {
        reply.error(Errno::EROFS);
    }

    fn symlink(
        &self,
        _req: &Request,
        _parent: INodeNo,
        _link_name: &OsStr,
        _target: &Path,
        reply: ReplyEntry,
    ) {
        reply.error(Errno::EROFS);
    }

    fn rename(
        &self,
        _req: &Request,
        _parent: INodeNo,
        _name: &OsStr,
        _newparent: INodeNo,
        _newname: &OsStr,
        _flags: RenameFlags,
        reply: ReplyEmpty,
    ) {
        reply.error(Errno::EROFS);
    }

    fn link(
        &self,
        _req: &Request,
        _ino: INodeNo,
        _newparent: INodeNo,
        _newname: &OsStr,
        reply: ReplyEntry,
    ) {
        reply.error(Errno::EROFS);
    }

    fn write(
        &self,
        _req: &Request,
        _ino: INodeNo,
        _fh: FileHandle,
        _offset: u64,
        _data: &[u8],
        _write_flags: WriteFlags,
        _flags: OpenFlags,
        _lock_owner: Option<LockOwner>,
        reply: ReplyWrite,
    ) {
        reply.error(Errno::EROFS);
    }

    fn setxattr(
        &self,
        _req: &Request,
        _ino: INodeNo,
        _name: &OsStr,
        _value: &[u8],
        _flags: i32,
        _position: u32,
        reply: ReplyEmpty,
    ) {
        reply.error(Errno::EROFS);
    }

    fn removexattr(&self, _req: &Request, _ino: INodeNo, _name: &OsStr, reply: ReplyEmpty) {
        reply.error(Errno::EROFS);
    }

    fn create(
        &self,
        _req: &Request,
        _parent: INodeNo,
        _name: &OsStr,
        _mode: u32,
        _umask: u32,
        _flags: i32,
        reply: ReplyCreate,
    ) {
        reply.error(Errno::EROFS);
    }

    fn fallocate(
        &self,
        _req: &Request,
        _ino: INodeNo,
        _fh: FileHandle,
        _offset: u64,
        _length: u64,
        _mode: i32,
        reply: ReplyEmpty,
    ) {
        reply.error(Errno::EROFS);
    }

    fn copy_file_range(
        &self,
        _req: &Request,
        _ino_in: INodeNo,
        _fh_in: FileHandle,
        _offset_in: u64,
        _ino_out: INodeNo,
        _fh_out: FileHandle,
        _offset_out: u64,
        _len: u64,
        _flags: CopyFileRangeFlags,
        reply: ReplyWrite,
    ) {
        reply.error(Errno::EROFS);
    }
}

fn read_at(content: &[u8], offset: u64, size: u32) -> &[u8] {
    let Ok(start) = usize::try_from(offset) else {
        return &[];
    };
    if start >= content.len() {
        return &[];
    }
    let end = start.saturating_add(size as usize).min(content.len());
    &content[start..end]
}

fn is_read_only_open(flags: i32) -> bool {
    let access_is_read_only = OpenFlags(flags).acc_mode() == OpenAccMode::O_RDONLY;
    let mutation_flags = libc::O_CREAT | libc::O_EXCL | libc::O_TRUNC | libc::O_APPEND;
    access_is_read_only && flags & mutation_flags == 0
}

extern "C" fn request_shutdown(_signal: libc::c_int) {
    SHUTDOWN_REQUESTED.store(true, Ordering::Relaxed);
}

fn install_signal_handlers() -> io::Result<()> {
    let action = libc::sigaction {
        sa_sigaction: request_shutdown as *const () as usize,
        sa_mask: unsafe { std::mem::zeroed() },
        sa_flags: 0,
        sa_restorer: None,
    };
    for signal in [libc::SIGINT, libc::SIGTERM] {
        if unsafe { libc::sigaction(signal, &action, std::ptr::null_mut()) } != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

fn usage(program: &OsStr) {
    eprintln!(
        "Usage: {} [MOUNT_PATH]\n\
         Default: $XDG_RUNTIME_DIR/tuxstack-fuse-poc/mnt\n\
         Concurrency probe (3 second delayed read, hidden from readdir):\n\
           cat MOUNT_PATH/{SLOW_NAME} & time cat MOUNT_PATH/{HELLO_NAME}",
        program.to_string_lossy()
    );
}

fn mount_path() -> Result<PathBuf, String> {
    let mut args = env::args_os();
    let program = args.next().unwrap_or_else(|| "fuse-readonly".into());
    let first = args.next();
    if first.as_deref() == Some(OsStr::new("-h")) || first.as_deref() == Some(OsStr::new("--help"))
    {
        usage(&program);
        return Err(String::new());
    }
    if args.next().is_some() {
        usage(&program);
        return Err("expected at most one mount path".to_string());
    }
    if let Some(path) = first {
        return Ok(PathBuf::from(path));
    }

    let runtime = env::var_os("XDG_RUNTIME_DIR")
        .ok_or_else(|| "XDG_RUNTIME_DIR is not set and no mount path was supplied".to_string())?;
    Ok(PathBuf::from(runtime).join("tuxstack-fuse-poc/mnt"))
}

fn prepare_mountpoint(path: &Path) -> io::Result<()> {
    if path.exists() {
        if !path.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("mount path is not a directory: {}", path.display()),
            ));
        }
        return Ok(());
    }

    fs::create_dir_all(path)?;
    if let Some(parent) = path.parent() {
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

fn run() -> Result<(), String> {
    let mountpoint = mount_path()?;
    prepare_mountpoint(&mountpoint)
        .map_err(|error| format!("cannot prepare {}: {error}", mountpoint.display()))?;
    install_signal_handlers()
        .map_err(|error| format!("cannot install signal handlers: {error}"))?;

    let fs = ReadOnlyFs {
        uid: unsafe { libc::geteuid() },
        gid: unsafe { libc::getegid() },
    };
    let mut config = Config::default();
    config.mount_options = vec![
        MountOption::RO,
        MountOption::DefaultPermissions,
        MountOption::NoDev,
        MountOption::NoSuid,
        MountOption::NoExec,
        MountOption::FSName("tuxstack-fuse-poc".to_string()),
    ];
    config.n_threads = Some(4);
    // Each worker gets a cloned /dev/fuse fd on Linux. A delayed callback can
    // therefore run while another worker services an unrelated hello.txt read.
    config.clone_fd = true;

    let session = fuser::spawn_mount(fs, &mountpoint, &config)
        .map_err(|error| format!("cannot mount {}: {error}", mountpoint.display()))?;
    eprintln!(
        "mounted read-only at {} (SIGINT/SIGTERM to unmount)",
        mountpoint.display()
    );

    while !SHUTDOWN_REQUESTED.load(Ordering::Relaxed) && !session.guard.is_finished() {
        thread::sleep(Duration::from_millis(50));
    }
    session.umount_and_join().map_err(|error| {
        format!(
            "unmount/session failure at {}: {error}",
            mountpoint.display()
        )
    })?;
    eprintln!("unmounted {}", mountpoint.display());
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) if error.is_empty() => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{HELLO, is_read_only_open, read_at};

    #[test]
    fn read_respects_offset_size_and_eof() {
        assert_eq!(read_at(HELLO, 0, 3), b"tux");
        assert_eq!(read_at(HELLO, 9, 64), b"fuse-poc\n");
        assert_eq!(read_at(HELLO, HELLO.len() as u64, 1), b"");
        assert_eq!(read_at(HELLO, u64::MAX, u32::MAX), b"");
        assert_eq!(read_at(HELLO, 2, 0), b"");
    }

    #[test]
    fn only_non_mutating_read_opens_are_allowed() {
        assert!(is_read_only_open(libc::O_RDONLY));
        assert!(is_read_only_open(libc::O_RDONLY | libc::O_CLOEXEC));
        assert!(!is_read_only_open(libc::O_WRONLY));
        assert!(!is_read_only_open(libc::O_RDWR));
        assert!(!is_read_only_open(libc::O_RDONLY | libc::O_TRUNC));
        assert!(!is_read_only_open(libc::O_RDONLY | libc::O_APPEND));
        assert!(!is_read_only_open(libc::O_RDONLY | libc::O_CREAT));
    }
}
