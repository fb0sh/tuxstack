use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime};

use fuser::{
    AccessFlags, BackgroundSession, BsdFileFlags, Config, CopyFileRangeFlags, Errno, FileAttr,
    FileHandle, FileType, Filesystem, FopenFlags, Generation, INodeNo, LockOwner, MountOption,
    OpenAccMode, OpenFlags, RenameFlags, ReplyAttr, ReplyCreate, ReplyData, ReplyDirectory,
    ReplyEmpty, ReplyEntry, ReplyOpen, ReplyStatfs, ReplyWrite, ReplyXattr, Request, TimeOrNow,
    WriteFlags,
};

use crate::{
    DockerFilesystemResource, HardlinkKey, InodeTable, OpenHandle, OpenHandleId, OpenHandleTable,
    ProviderExecutor, ReadOnlyFilesystemProvider, RequestContext, VfsError, VirtualFileName,
    VirtualFileType, VirtualMetadata, VirtualNodeKey, VirtualPath, rewrite_symlink,
};

const TTL: Duration = Duration::from_secs(1);

#[derive(Clone, Debug)]
struct CachedNode {
    path: VirtualPath,
    metadata: VirtualMetadata,
}

#[derive(Clone, Debug, Default)]
pub struct InvalidationNotifier {
    notifier: Arc<RwLock<Option<fuser::Notifier>>>,
}

impl InvalidationNotifier {
    pub fn attach(&self, notifier: fuser::Notifier) {
        *self.notifier.write().expect("notifier lock poisoned") = Some(notifier);
    }

    pub fn invalidate_inode(&self, inode: u64) -> std::io::Result<()> {
        if let Some(notifier) = self
            .notifier
            .read()
            .expect("notifier lock poisoned")
            .as_ref()
        {
            notifier.inval_inode(INodeNo(inode), 0, 0)
        } else {
            Ok(())
        }
    }

    pub fn invalidate_entry(&self, parent: u64, name: &VirtualFileName) -> std::io::Result<()> {
        if let Some(notifier) = self
            .notifier
            .read()
            .expect("notifier lock poisoned")
            .as_ref()
        {
            notifier.inval_entry(INodeNo(parent), name.as_os_str())
        } else {
            Ok(())
        }
    }

    pub fn delete_entry(
        &self,
        parent: u64,
        child: u64,
        name: &VirtualFileName,
    ) -> std::io::Result<()> {
        if let Some(notifier) = self
            .notifier
            .read()
            .expect("notifier lock poisoned")
            .as_ref()
        {
            notifier.delete(INodeNo(parent), INodeNo(child), name.as_os_str())
        } else {
            Ok(())
        }
    }
}

pub struct ReadOnlyFuseAdapter {
    provider: Arc<dyn ReadOnlyFilesystemProvider>,
    resource: DockerFilesystemResource,
    daemon_identity: String,
    provider_key: String,
    uid: u32,
    gid: u32,
    executor: ProviderExecutor,
    inodes: Mutex<InodeTable>,
    nodes: RwLock<HashMap<u64, CachedNode>>,
    handles: Mutex<OpenHandleTable>,
    next_request_id: AtomicU64,
    notifier: InvalidationNotifier,
}

impl ReadOnlyFuseAdapter {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        provider: Arc<dyn ReadOnlyFilesystemProvider>,
        resource: DockerFilesystemResource,
        daemon_identity: impl Into<String>,
        provider_key: impl Into<String>,
        uid: u32,
        gid: u32,
        operation_timeout: Duration,
        max_provider_operations: usize,
        max_handles: usize,
        handle_idle_timeout: Duration,
    ) -> Result<Self, VfsError> {
        let root = VirtualPath::root();
        let context = RequestContext {
            uid,
            gid,
            pid: 0,
            request_id: 0,
        };
        let executor = ProviderExecutor::new(max_provider_operations, operation_timeout)?;
        let provider_for_root = Arc::clone(&provider);
        let root_for_provider = root.clone();
        let root_metadata = executor.execute(async move {
            provider_for_root
                .getattr(&root_for_provider, &context)
                .await
        })?;
        if root_metadata.file_type != VirtualFileType::Directory {
            return Err(VfsError::InvalidInput("provider root must be a directory"));
        }
        Ok(Self {
            provider,
            resource,
            daemon_identity: daemon_identity.into(),
            provider_key: provider_key.into(),
            uid,
            gid,
            executor,
            inodes: Mutex::new(InodeTable::new()),
            nodes: RwLock::new(HashMap::from([(
                crate::ROOT_INODE,
                CachedNode {
                    path: root,
                    metadata: root_metadata,
                },
            )])),
            handles: Mutex::new(OpenHandleTable::new(max_handles, handle_idle_timeout)?),
            next_request_id: AtomicU64::new(1),
            notifier: InvalidationNotifier::default(),
        })
    }

    pub fn notifier(&self) -> InvalidationNotifier {
        self.notifier.clone()
    }

    pub fn mount_config() -> Config {
        let mut config = Config::default();
        config.mount_options = vec![
            MountOption::RO,
            // Do not use DefaultPermissions: the kernel would reject writes to 0444
            // nodes as EACCES before the adapter can return the required EROFS.
            MountOption::NoDev,
            MountOption::NoSuid,
            MountOption::NoExec,
            MountOption::FSName("tuxstack-vfs".to_owned()),
        ];
        config.n_threads = Some(4);
        config.clone_fd = cfg!(target_os = "linux");
        config
    }

    pub fn spawn_mount(self, mountpoint: impl AsRef<Path>) -> std::io::Result<BackgroundSession> {
        let notifier = self.notifier();
        let session = fuser::spawn_mount(self, mountpoint, &Self::mount_config())?;
        notifier.attach(session.notifier());
        Ok(session)
    }

    fn context(&self, request: &Request) -> RequestContext {
        RequestContext {
            uid: request.uid(),
            gid: request.gid(),
            pid: request.pid(),
            request_id: self.next_request_id.fetch_add(1, Ordering::Relaxed),
        }
    }

    fn node(&self, inode: u64) -> Result<CachedNode, VfsError> {
        self.nodes
            .read()
            .map_err(|_| VfsError::Io("node cache lock poisoned".to_owned()))?
            .get(&inode)
            .cloned()
            .ok_or(VfsError::NotFound)
    }

    fn cache_node(&self, path: VirtualPath, metadata: VirtualMetadata) -> Result<u64, VfsError> {
        let logical_path = path.clone();
        let key = VirtualNodeKey {
            daemon_identity: self.daemon_identity.clone(),
            resource_kind: self.resource.kind_name().to_owned(),
            resource_id: self.resource.canonical_id().to_owned(),
            provider_key: self.provider_key.clone(),
            logical_path,
            generation: metadata.generation,
        };
        let hardlink_key = Some(HardlinkKey {
            daemon_identity: self.daemon_identity.clone(),
            resource_kind: self.resource.kind_name().to_owned(),
            resource_id: self.resource.canonical_id().to_owned(),
            provider_key: self.provider_key.clone(),
            provider_node_id: metadata.node_id.clone(),
            generation: metadata.generation,
        });
        let inode = self
            .inodes
            .lock()
            .map_err(|_| VfsError::Io("inode table lock poisoned".to_owned()))?
            .inode_for(key, hardlink_key)?;
        self.nodes
            .write()
            .map_err(|_| VfsError::Io("node cache lock poisoned".to_owned()))?
            .insert(inode, CachedNode { path, metadata });
        Ok(inode)
    }

    fn attr(&self, inode: u64, metadata: &VirtualMetadata) -> FileAttr {
        let mode = match metadata.file_type {
            VirtualFileType::Directory => 0o555,
            VirtualFileType::RegularFile => 0o444,
            VirtualFileType::Symlink => 0o777,
            _ => 0o444,
        };
        FileAttr {
            ino: INodeNo(inode),
            size: metadata.size,
            blocks: metadata.size.div_ceil(512),
            atime: metadata.mtime,
            mtime: metadata.mtime,
            ctime: metadata.mtime,
            crtime: SystemTime::UNIX_EPOCH,
            kind: file_type(metadata.file_type),
            perm: mode,
            nlink: metadata.nlink.max(1),
            uid: self.uid,
            gid: self.gid,
            rdev: u32::try_from(metadata.device_id.unwrap_or(0)).unwrap_or(0),
            blksize: 4096,
            flags: 0,
        }
    }

    fn execute<T, F>(&self, future: F) -> Result<T, VfsError>
    where
        F: std::future::Future<Output = Result<T, VfsError>> + Send + 'static,
        T: Send + 'static,
    {
        self.executor.execute(future)
    }

    fn original_xattrs(&self, metadata: &VirtualMetadata) -> Vec<(&'static str, String)> {
        vec![
            (
                "user.tuxstack.original_uid",
                metadata.original.uid.to_string(),
            ),
            (
                "user.tuxstack.original_gid",
                metadata.original.gid.to_string(),
            ),
            (
                "user.tuxstack.original_mode",
                format!("{:04o}", metadata.original.mode & 0o7777),
            ),
            (
                "user.tuxstack.provider",
                format!("{:?}", self.provider.descriptor().kind),
            ),
            (
                "user.tuxstack.resource_id",
                self.resource.canonical_id().to_owned(),
            ),
        ]
    }
}

impl Filesystem for ReadOnlyFuseAdapter {
    fn lookup(&self, request: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEntry) {
        let result = (|| {
            let parent = self.node(parent.0)?;
            let name = VirtualFileName::try_from(name)?;
            let path = parent.path.join(&name)?;
            let provider = Arc::clone(&self.provider);
            let provider_parent = parent.path;
            let provider_name = name;
            let context = self.context(request);
            let metadata = self.execute(async move {
                provider
                    .lookup(&provider_parent, &provider_name, &context)
                    .await
            })?;
            let inode = self.cache_node(path, metadata.clone())?;
            Ok((inode, metadata))
        })();
        match result {
            Ok((inode, metadata)) => reply.entry(
                &TTL,
                &self.attr(inode, &metadata),
                Generation(metadata.generation),
            ),
            Err(error) => reply.error(errno(&error)),
        }
    }

    fn forget(&self, _request: &Request, inode: INodeNo, lookup_count: u64) {
        if let Ok(mut inodes) = self.inodes.lock() {
            inodes.forget(inode.0, lookup_count);
        }
    }

    fn getattr(
        &self,
        request: &Request,
        inode: INodeNo,
        _handle: Option<FileHandle>,
        reply: ReplyAttr,
    ) {
        let result = (|| {
            let node = self.node(inode.0)?;
            let provider = Arc::clone(&self.provider);
            let path = node.path.clone();
            let context = self.context(request);
            let metadata = self.execute(async move { provider.getattr(&path, &context).await })?;
            self.nodes
                .write()
                .map_err(|_| VfsError::Io("node cache lock poisoned".to_owned()))?
                .insert(
                    inode.0,
                    CachedNode {
                        path: node.path,
                        metadata: metadata.clone(),
                    },
                );
            Ok(metadata)
        })();
        match result {
            Ok(metadata) => reply.attr(&TTL, &self.attr(inode.0, &metadata)),
            Err(error) => reply.error(errno(&error)),
        }
    }

    fn readlink(&self, request: &Request, inode: INodeNo, reply: ReplyData) {
        let result = (|| {
            let node = self.node(inode.0)?;
            if node.metadata.file_type != VirtualFileType::Symlink {
                return Err(VfsError::InvalidInput("node is not a symlink"));
            }
            let provider = Arc::clone(&self.provider);
            let path = node.path.clone();
            let context = self.context(request);
            let target = self.execute(async move { provider.read_link(&path, &context).await })?;
            rewrite_symlink(&node.path, &target)
        })();
        match result {
            Ok(target) => reply.data(target.as_bytes()),
            Err(error) => reply.error(errno(&error)),
        }
    }

    fn open(&self, request: &Request, inode: INodeNo, flags: OpenFlags, reply: ReplyOpen) {
        let result = (|| {
            if !is_read_only_open(flags.0) {
                return Err(VfsError::ReadOnly);
            }
            let node = self.node(inode.0)?;
            match node.metadata.file_type {
                VirtualFileType::Directory => return Err(VfsError::IsDirectory),
                VirtualFileType::RegularFile => {}
                VirtualFileType::Symlink => {
                    return Err(VfsError::InvalidInput("cannot open symlink directly"));
                }
                _ => return Err(VfsError::SpecialFile),
            }
            let provider = Arc::clone(&self.provider);
            let path = node.path.clone();
            let context = self.context(request);
            let provider_handle =
                self.execute(async move { provider.open(&path, flags.0, &context).await })?;
            let handle = OpenHandle {
                provider: Arc::clone(&self.provider),
                provider_handle,
                resource: self.resource.clone(),
                path: node.path,
                content_generation: node.metadata.generation,
                backing_strategy: "provider".to_owned(),
                opened_at: std::time::Instant::now(),
                last_accessed_at: std::time::Instant::now(),
            };
            self.handles
                .lock()
                .map_err(|_| VfsError::Io("handle table lock poisoned".to_owned()))?
                .insert(handle)
        })();
        match result {
            Ok(handle) => reply.opened(FileHandle(handle.0), FopenFlags::empty()),
            Err(error) => reply.error(errno(&error)),
        }
    }

    fn read(
        &self,
        request: &Request,
        _inode: INodeNo,
        handle: FileHandle,
        offset: u64,
        size: u32,
        _flags: OpenFlags,
        _lock_owner: Option<LockOwner>,
        reply: ReplyData,
    ) {
        let result = (|| {
            let (provider, provider_handle) = {
                let mut handles = self
                    .handles
                    .lock()
                    .map_err(|_| VfsError::Io("handle table lock poisoned".to_owned()))?;
                let handle = handles.get_mut(OpenHandleId(handle.0))?;
                (Arc::clone(&handle.provider), handle.provider_handle.clone())
            };
            let context = self.context(request);
            self.execute(async move {
                provider
                    .read_at(&provider_handle, offset, size, &context)
                    .await
            })
        })();
        match result {
            Ok(data) => reply.data(&data),
            Err(error) => reply.error(errno(&error)),
        }
    }

    fn release(
        &self,
        _request: &Request,
        _inode: INodeNo,
        handle: FileHandle,
        _flags: OpenFlags,
        _lock_owner: Option<LockOwner>,
        _flush: bool,
        reply: ReplyEmpty,
    ) {
        let result = (|| {
            let handle = self
                .handles
                .lock()
                .map_err(|_| VfsError::Io("handle table lock poisoned".to_owned()))?
                .remove(OpenHandleId(handle.0))?;
            let provider = handle.provider;
            let provider_handle = handle.provider_handle;
            self.execute(async move { provider.close(provider_handle).await })
        })();
        match result {
            Ok(()) => reply.ok(),
            Err(error) => reply.error(errno(&error)),
        }
    }

    fn readdir(
        &self,
        request: &Request,
        inode: INodeNo,
        _handle: FileHandle,
        offset: u64,
        mut reply: ReplyDirectory,
    ) {
        let result = (|| {
            let node = self.node(inode.0)?;
            if node.metadata.file_type != VirtualFileType::Directory {
                return Err(VfsError::NotDirectory);
            }
            let provider = Arc::clone(&self.provider);
            let path = node.path.clone();
            let context = self.context(request);
            let entries = self.execute(async move { provider.read_dir(&path, &context).await })?;
            let parent_inode = node
                .path
                .parent()
                .and_then(|parent_path| {
                    self.nodes.read().ok()?.iter().find_map(|(ino, candidate)| {
                        (candidate.path == parent_path).then_some(*ino)
                    })
                })
                .unwrap_or(crate::ROOT_INODE);
            let mut resolved = vec![
                (inode.0, VirtualFileType::Directory, b".".to_vec()),
                (parent_inode, VirtualFileType::Directory, b"..".to_vec()),
            ];
            for entry in entries.iter() {
                let child_path = node.path.join(&entry.name)?;
                let child_inode = self.cache_node(child_path, entry.metadata.clone())?;
                resolved.push((
                    child_inode,
                    entry.metadata.file_type,
                    entry.name.as_bytes().to_vec(),
                ));
            }
            Ok(resolved)
        })();
        match result {
            Ok(entries) => {
                let start = usize::try_from(offset).unwrap_or(entries.len());
                for (index, (entry_inode, kind, name)) in entries.iter().enumerate().skip(start) {
                    if reply.add(
                        INodeNo(*entry_inode),
                        (index + 1) as u64,
                        file_type(*kind),
                        OsStr::from_bytes(name),
                    ) {
                        break;
                    }
                }
                reply.ok();
            }
            Err(error) => reply.error(errno(&error)),
        }
    }

    fn statfs(&self, _request: &Request, _inode: INodeNo, reply: ReplyStatfs) {
        // Virtual namespace semantics: this is not Docker storage accounting. Report
        // one allocated namespace block and zero writable/free capacity.
        reply.statfs(1, 0, 0, 1, 0, 4096, 255, 4096);
    }

    fn access(&self, _request: &Request, inode: INodeNo, mask: AccessFlags, reply: ReplyEmpty) {
        match self.node(inode.0) {
            Err(error) => reply.error(errno(&error)),
            Ok(_) if mask.contains(AccessFlags::W_OK) => reply.error(Errno::EROFS),
            Ok(node)
                if mask.contains(AccessFlags::X_OK)
                    && node.metadata.file_type != VirtualFileType::Directory =>
            {
                reply.error(Errno::EACCES)
            }
            Ok(_) => reply.ok(),
        }
    }

    fn getxattr(
        &self,
        _request: &Request,
        inode: INodeNo,
        name: &OsStr,
        size: u32,
        reply: ReplyXattr,
    ) {
        let result = self.node(inode.0).and_then(|node| {
            self.original_xattrs(&node.metadata)
                .into_iter()
                .find(|(candidate, _)| name == OsStr::new(candidate))
                .map(|(_, value)| value.into_bytes())
                .ok_or(VfsError::NotFound)
        });
        xattr_reply(result, size, reply);
    }

    fn listxattr(&self, _request: &Request, inode: INodeNo, size: u32, reply: ReplyXattr) {
        let result = self.node(inode.0).map(|node| {
            let mut result = Vec::new();
            for (name, _) in self.original_xattrs(&node.metadata) {
                result.extend_from_slice(name.as_bytes());
                result.push(0);
            }
            result
        });
        xattr_reply(result, size, reply);
    }

    fn setattr(
        &self,
        _request: &Request,
        _inode: INodeNo,
        _mode: Option<u32>,
        _uid: Option<u32>,
        _gid: Option<u32>,
        _size: Option<u64>,
        _atime: Option<TimeOrNow>,
        _mtime: Option<TimeOrNow>,
        _ctime: Option<SystemTime>,
        _handle: Option<FileHandle>,
        _crtime: Option<SystemTime>,
        _chgtime: Option<SystemTime>,
        _bkuptime: Option<SystemTime>,
        _flags: Option<BsdFileFlags>,
        reply: ReplyAttr,
    ) {
        reply.error(Errno::EROFS);
    }

    fn mknod(&self, _: &Request, _: INodeNo, _: &OsStr, _: u32, _: u32, _: u32, reply: ReplyEntry) {
        reply.error(Errno::EROFS);
    }
    fn mkdir(&self, _: &Request, _: INodeNo, _: &OsStr, _: u32, _: u32, reply: ReplyEntry) {
        reply.error(Errno::EROFS);
    }
    fn unlink(&self, _: &Request, _: INodeNo, _: &OsStr, reply: ReplyEmpty) {
        reply.error(Errno::EROFS);
    }
    fn rmdir(&self, _: &Request, _: INodeNo, _: &OsStr, reply: ReplyEmpty) {
        reply.error(Errno::EROFS);
    }
    fn symlink(&self, _: &Request, _: INodeNo, _: &OsStr, _: &Path, reply: ReplyEntry) {
        reply.error(Errno::EROFS);
    }
    fn rename(
        &self,
        _: &Request,
        _: INodeNo,
        _: &OsStr,
        _: INodeNo,
        _: &OsStr,
        _: RenameFlags,
        reply: ReplyEmpty,
    ) {
        reply.error(Errno::EROFS);
    }
    fn link(&self, _: &Request, _: INodeNo, _: INodeNo, _: &OsStr, reply: ReplyEntry) {
        reply.error(Errno::EROFS);
    }
    fn write(
        &self,
        _: &Request,
        _: INodeNo,
        _: FileHandle,
        _: u64,
        _: &[u8],
        _: WriteFlags,
        _: OpenFlags,
        _: Option<LockOwner>,
        reply: ReplyWrite,
    ) {
        reply.error(Errno::EROFS);
    }
    fn setxattr(
        &self,
        _: &Request,
        _: INodeNo,
        _: &OsStr,
        _: &[u8],
        _: i32,
        _: u32,
        reply: ReplyEmpty,
    ) {
        reply.error(Errno::EROFS);
    }
    fn removexattr(&self, _: &Request, _: INodeNo, _: &OsStr, reply: ReplyEmpty) {
        reply.error(Errno::EROFS);
    }
    fn create(
        &self,
        _: &Request,
        _: INodeNo,
        _: &OsStr,
        _: u32,
        _: u32,
        _: i32,
        reply: ReplyCreate,
    ) {
        reply.error(Errno::EROFS);
    }
    fn fallocate(
        &self,
        _: &Request,
        _: INodeNo,
        _: FileHandle,
        _: u64,
        _: u64,
        _: i32,
        reply: ReplyEmpty,
    ) {
        reply.error(Errno::EROFS);
    }
    fn copy_file_range(
        &self,
        _: &Request,
        _: INodeNo,
        _: FileHandle,
        _: u64,
        _: INodeNo,
        _: FileHandle,
        _: u64,
        _: u64,
        _: CopyFileRangeFlags,
        reply: ReplyWrite,
    ) {
        reply.error(Errno::EROFS);
    }
}

pub fn is_read_only_open(flags: i32) -> bool {
    let access_is_read_only = OpenFlags(flags).acc_mode() == OpenAccMode::O_RDONLY;
    let mutation_flags = libc::O_CREAT | libc::O_EXCL | libc::O_TRUNC | libc::O_APPEND;
    access_is_read_only && flags & mutation_flags == 0
}

fn file_type(file_type: VirtualFileType) -> FileType {
    match file_type {
        VirtualFileType::RegularFile => FileType::RegularFile,
        VirtualFileType::Directory => FileType::Directory,
        VirtualFileType::Symlink => FileType::Symlink,
        VirtualFileType::CharacterDevice => FileType::CharDevice,
        VirtualFileType::BlockDevice => FileType::BlockDevice,
        VirtualFileType::NamedPipe => FileType::NamedPipe,
        VirtualFileType::Socket => FileType::Socket,
    }
}

fn errno(error: &VfsError) -> Errno {
    Errno::from_i32(error.errno())
}

fn xattr_reply(result: Result<Vec<u8>, VfsError>, size: u32, reply: ReplyXattr) {
    match result {
        Err(error) => reply.error(errno(&error)),
        Ok(value) if size == 0 => reply.size(value.len() as u32),
        Ok(value) if value.len() > size as usize => reply.error(Errno::ERANGE),
        Ok(value) => reply.data(&value),
    }
}

use std::os::unix::ffi::OsStrExt;
