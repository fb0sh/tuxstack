//! Bounded, metadata-only streaming tar indexing for Docker exports.
//!
//! Paths and link targets remain byte strings.  No file body is retained while
//! indexing, and no host path is ever constructed from archive input.

use std::collections::{BTreeMap, BTreeSet};
use std::pin::Pin;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bytes::{Bytes, BytesMut};
use futures_util::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use tuxstack_vfs::{
    OriginalMetadata, VfsError, VirtualDirectoryEntry, VirtualFileName, VirtualFileType,
    VirtualMetadata, VirtualPath, VirtualPathBytes,
};

pub type ArchiveByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, VfsError>> + Send + 'static>>;

const TAR_BLOCK: usize = 512;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TarLimits {
    pub max_archive_bytes: u64,
    pub max_entries: usize,
    pub max_path_bytes: usize,
    pub max_extension_bytes: usize,
    pub max_entry_bytes: u64,
}

impl Default for TarLimits {
    fn default() -> Self {
        Self {
            max_archive_bytes: 8 * 1024 * 1024 * 1024,
            max_entries: 100_000,
            max_path_bytes: 16 * 1024,
            max_extension_bytes: 1024 * 1024,
            max_entry_bytes: 2 * 1024 * 1024 * 1024,
        }
    }
}

impl TarLimits {
    pub fn validate(&self) -> Result<(), VfsError> {
        if self.max_archive_bytes < (TAR_BLOCK * 2) as u64
            || self.max_entries == 0
            || self.max_path_bytes == 0
            || self.max_extension_bytes == 0
            || self.max_entry_bytes == 0
            || self.max_entry_bytes > self.max_archive_bytes
        {
            return Err(VfsError::InvalidInput("invalid tar resource limits"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub struct TarPath {
    components: Vec<Vec<u8>>,
}

impl TarPath {
    pub fn root() -> Self {
        Self {
            components: Vec::new(),
        }
    }

    pub fn from_tar_bytes(raw: &[u8], limits: &TarLimits) -> Result<Self, VfsError> {
        if raw.len() > limits.max_path_bytes {
            return Err(VfsError::PathTooLong);
        }
        if raw.first() == Some(&b'/') {
            return Err(VfsError::InvalidInput("absolute tar entry path"));
        }
        if raw.contains(&0) {
            return Err(VfsError::InvalidInput("NUL in tar entry path"));
        }

        let mut components = Vec::new();
        for component in raw.split(|byte| *byte == b'/') {
            match component {
                b"" | b"." => {}
                b".." => return Err(VfsError::SymlinkEscape),
                component => {
                    components.push(VirtualFileName::new(component)?.as_bytes().to_vec());
                }
            }
        }
        let path = Self { components };
        if path.encoded_len() > limits.max_path_bytes {
            return Err(VfsError::PathTooLong);
        }
        Ok(path)
    }

    pub fn from_virtual(path: &VirtualPath) -> Self {
        Self {
            components: path
                .components()
                .iter()
                .map(|component| component.as_bytes().to_vec())
                .collect(),
        }
    }

    pub fn to_virtual(&self) -> Result<VirtualPath, VfsError> {
        VirtualPath::from_components(
            self.components
                .iter()
                .map(VirtualFileName::new)
                .collect::<Result<Vec<_>, _>>()?,
        )
    }

    pub fn components(&self) -> &[Vec<u8>] {
        &self.components
    }

    pub fn is_root(&self) -> bool {
        self.components.is_empty()
    }

    pub fn depth(&self) -> usize {
        self.components.len()
    }

    pub fn parent(&self) -> Option<Self> {
        (!self.is_root()).then(|| Self {
            components: self.components[..self.components.len() - 1].to_vec(),
        })
    }

    pub fn name(&self) -> Option<&[u8]> {
        self.components.last().map(Vec::as_slice)
    }

    pub fn starts_with(&self, prefix: &Self) -> bool {
        self.components.starts_with(&prefix.components)
    }

    pub fn is_descendant_of(&self, parent: &Self) -> bool {
        self.depth() > parent.depth() && self.starts_with(parent)
    }

    fn encoded_len(&self) -> usize {
        if self.is_root() {
            1
        } else {
            self.components.iter().map(|item| item.len() + 1).sum()
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum TarEntryKind {
    RegularFile,
    Directory,
    Symlink,
    Hardlink,
    CharacterDevice,
    BlockDevice,
    NamedPipe,
    Other,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TarEntry {
    /// Header/PAX/GNU path exactly as supplied, minus GNU trailing NUL/newline.
    pub raw_path: Vec<u8>,
    pub path: TarPath,
    pub kind: TarEntryKind,
    pub size: u64,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    pub mtime: i64,
    /// Kept as byte metadata. Absolute targets are intentionally not rewritten
    /// here; the FUSE symlink layer performs resource-root-relative rewriting.
    pub link_target: Option<Vec<u8>>,
    pub device_major: Option<u32>,
    pub device_minor: Option<u32>,
    pub synthetic: bool,
}

impl TarEntry {
    pub fn root() -> Self {
        Self {
            raw_path: b".".to_vec(),
            path: TarPath::root(),
            kind: TarEntryKind::Directory,
            size: 0,
            mode: 0o755,
            uid: 0,
            gid: 0,
            mtime: 0,
            link_target: None,
            device_major: None,
            device_minor: None,
            synthetic: true,
        }
    }

    pub fn synthetic_directory(path: TarPath) -> Self {
        let raw_path = path.components().iter().enumerate().fold(
            Vec::new(),
            |mut bytes, (index, component)| {
                if index != 0 {
                    bytes.push(b'/');
                }
                bytes.extend_from_slice(component);
                bytes
            },
        );
        Self {
            raw_path,
            path,
            kind: TarEntryKind::Directory,
            size: 0,
            mode: 0o755,
            uid: 0,
            gid: 0,
            mtime: 0,
            link_target: None,
            device_major: None,
            device_minor: None,
            synthetic: true,
        }
    }

    pub fn metadata(&self, identity_prefix: &[u8], generation: u64) -> VirtualMetadata {
        let mut node_id = Vec::with_capacity(identity_prefix.len() + self.raw_path.len() + 2);
        node_id.extend_from_slice(identity_prefix);
        node_id.push(0);
        node_id.extend_from_slice(&self.raw_path);
        let file_type = match self.kind {
            TarEntryKind::RegularFile | TarEntryKind::Hardlink => VirtualFileType::RegularFile,
            TarEntryKind::Directory => VirtualFileType::Directory,
            TarEntryKind::Symlink => VirtualFileType::Symlink,
            TarEntryKind::CharacterDevice => VirtualFileType::CharacterDevice,
            TarEntryKind::BlockDevice => VirtualFileType::BlockDevice,
            TarEntryKind::NamedPipe => VirtualFileType::NamedPipe,
            TarEntryKind::Other => VirtualFileType::Socket,
        };
        let device_id = match (self.device_major, self.device_minor) {
            (Some(major), Some(minor)) => Some((u64::from(major) << 32) | u64::from(minor)),
            _ => None,
        };
        VirtualMetadata {
            node_id,
            file_type,
            size: self.size,
            nlink: if self.kind == TarEntryKind::Hardlink {
                2
            } else {
                1
            },
            mtime: system_time(self.mtime),
            original: OriginalMetadata {
                mode: self.mode,
                uid: self.uid,
                gid: self.gid,
            },
            device_id,
            generation,
        }
    }
}

fn system_time(seconds: i64) -> SystemTime {
    if seconds >= 0 {
        UNIX_EPOCH + Duration::from_secs(seconds as u64)
    } else {
        UNIX_EPOCH
            .checked_sub(Duration::from_secs(seconds.unsigned_abs()))
            .unwrap_or(UNIX_EPOCH)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TarIndex {
    #[serde(with = "entry_map_serde")]
    entries: BTreeMap<TarPath, TarEntry>,
}

mod entry_map_serde {
    use std::collections::BTreeMap;

    use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error};

    use super::{TarEntry, TarPath};

    pub fn serialize<S>(
        entries: &BTreeMap<TarPath, TarEntry>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        entries.values().collect::<Vec<_>>().serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<BTreeMap<TarPath, TarEntry>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let entries = Vec::<TarEntry>::deserialize(deserializer)?;
        let mut map = BTreeMap::new();
        for entry in entries {
            let path = entry.path.clone();
            if map.insert(path, entry).is_some() {
                return Err(D::Error::custom("duplicate persisted tar path"));
            }
        }
        Ok(map)
    }
}

impl TarIndex {
    pub fn empty() -> Self {
        Self {
            entries: BTreeMap::from([(TarPath::root(), TarEntry::root())]),
        }
    }

    pub async fn from_stream(
        stream: ArchiveByteStream,
        limits: TarLimits,
    ) -> Result<Self, VfsError> {
        limits.validate()?;
        let mut reader = TarStreamReader::new(stream, limits.clone());
        let mut index = Self::empty();
        while let Some(entry) = reader.next_entry().await? {
            reader.skip_entry_body().await?;
            if entry.path.is_root() {
                continue;
            }
            index.insert_with_parents(entry, limits.max_entries)?;
        }
        Ok(index)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.len() == 1 && self.entries.contains_key(&TarPath::root())
    }

    pub fn get(&self, path: &TarPath) -> Option<&TarEntry> {
        self.entries.get(path)
    }

    pub fn get_virtual(&self, path: &VirtualPath) -> Option<&TarEntry> {
        self.get(&TarPath::from_virtual(path))
    }

    pub fn metadata(
        &self,
        path: &TarPath,
        identity_prefix: &[u8],
        generation: u64,
    ) -> Result<VirtualMetadata, VfsError> {
        let entry = self.get(path).ok_or(VfsError::NotFound)?;
        if entry.kind != TarEntryKind::Hardlink {
            return Ok(entry.metadata(identity_prefix, generation));
        }
        let target = self.resolve_hardlink(path)?;
        let mut metadata = target.metadata(identity_prefix, generation);
        metadata.nlink = metadata.nlink.max(2);
        Ok(metadata)
    }

    pub fn content_path(&self, path: &VirtualPath) -> Result<VirtualPath, VfsError> {
        let path = TarPath::from_virtual(path);
        let entry = self.get(&path).ok_or(VfsError::NotFound)?;
        let content = if entry.kind == TarEntryKind::Hardlink {
            self.resolve_hardlink(&path)?
        } else {
            entry
        };
        match content.kind {
            TarEntryKind::RegularFile => content.path.to_virtual(),
            TarEntryKind::Directory => Err(VfsError::IsDirectory),
            TarEntryKind::CharacterDevice
            | TarEntryKind::BlockDevice
            | TarEntryKind::NamedPipe
            | TarEntryKind::Other => Err(VfsError::SpecialFile),
            _ => Err(VfsError::InvalidInput("node is not a regular file")),
        }
    }

    pub fn entries(&self) -> impl Iterator<Item = &TarEntry> {
        self.entries.values()
    }

    pub fn children(&self, parent: &TarPath) -> impl Iterator<Item = &TarEntry> {
        let expected_depth = parent.depth() + 1;
        self.entries.values().filter(move |entry| {
            entry.path.depth() == expected_depth && entry.path.parent().as_ref() == Some(parent)
        })
    }

    pub fn directory_entries(
        &self,
        parent: &VirtualPath,
        identity_prefix: &[u8],
        generation: u64,
    ) -> Result<Vec<VirtualDirectoryEntry>, VfsError> {
        let path = TarPath::from_virtual(parent);
        let entry = self.get(&path).ok_or(VfsError::NotFound)?;
        if entry.kind != TarEntryKind::Directory {
            return Err(VfsError::NotDirectory);
        }
        self.children(&path)
            .map(|entry| {
                Ok(VirtualDirectoryEntry {
                    name: VirtualFileName::new(entry.path.name().ok_or(VfsError::NotFound)?)?,
                    metadata: self.metadata(&entry.path, identity_prefix, generation)?,
                })
            })
            .collect()
    }

    fn resolve_hardlink(&self, path: &TarPath) -> Result<&TarEntry, VfsError> {
        let mut current = path.clone();
        let mut visited = BTreeSet::new();
        for _ in 0..40 {
            if !visited.insert(current.clone()) {
                return Err(VfsError::SymlinkLoop);
            }
            let entry = self.get(&current).ok_or(VfsError::NotFound)?;
            if entry.kind != TarEntryKind::Hardlink {
                return Ok(entry);
            }
            current = TarPath::from_tar_bytes(
                entry
                    .link_target
                    .as_deref()
                    .ok_or_else(|| VfsError::Io("hardlink has no target".into()))?,
                &TarLimits::default(),
            )?;
        }
        Err(VfsError::SymlinkLoop)
    }

    pub fn insert_synthetic_directory(
        &mut self,
        path: TarPath,
        max_entries: usize,
    ) -> Result<(), VfsError> {
        if path.is_root() {
            return Ok(());
        }
        self.insert_with_parents(TarEntry::synthetic_directory(path), max_entries)
    }

    pub fn remove_descendants(&mut self, root: &TarPath) {
        let remove: Vec<_> = self
            .entries
            .keys()
            .filter(|path| path.is_descendant_of(root))
            .cloned()
            .collect();
        for path in remove {
            self.entries.remove(&path);
        }
    }

    pub fn replace(&mut self, entry: TarEntry) {
        self.entries.insert(entry.path.clone(), entry);
    }

    pub fn validate_persisted(&self, limits: &TarLimits) -> Result<(), VfsError> {
        limits.validate()?;
        if self.entries.len() > limits.max_entries + 1
            || self.entries.get(&TarPath::root()).map(|entry| entry.kind)
                != Some(TarEntryKind::Directory)
        {
            return Err(VfsError::InvalidInput("corrupt tar index"));
        }
        for (path, entry) in &self.entries {
            if path != &entry.path || path.encoded_len() > limits.max_path_bytes {
                return Err(VfsError::InvalidInput("corrupt tar index path"));
            }
            if !path.is_root() && !self.entries.contains_key(&path.parent().expect("non-root")) {
                return Err(VfsError::InvalidInput("corrupt tar index parent"));
            }
            if entry.kind == TarEntryKind::Hardlink {
                TarPath::from_tar_bytes(
                    entry
                        .link_target
                        .as_deref()
                        .ok_or(VfsError::InvalidInput("corrupt hardlink target"))?,
                    limits,
                )?;
            }
        }
        Ok(())
    }

    fn insert_with_parents(&mut self, entry: TarEntry, max_entries: usize) -> Result<(), VfsError> {
        let mut missing = Vec::new();
        let mut parent = entry.path.parent();
        while let Some(path) = parent {
            if self.entries.contains_key(&path) {
                break;
            }
            missing.push(path.clone());
            parent = path.parent();
        }
        missing.reverse();
        for path in missing {
            self.ensure_capacity(max_entries)?;
            self.entries
                .insert(path.clone(), TarEntry::synthetic_directory(path));
        }
        if let Some(existing) = self.entries.get(&entry.path) {
            if existing.synthetic && !entry.synthetic {
                self.entries.insert(entry.path.clone(), entry);
                return Ok(());
            }
            return Err(VfsError::InvalidInput("duplicate tar entry path"));
        }
        self.ensure_capacity(max_entries)?;
        self.entries.insert(entry.path.clone(), entry);
        Ok(())
    }

    fn ensure_capacity(&self, max_entries: usize) -> Result<(), VfsError> {
        if self.entries.len().saturating_sub(1) >= max_entries {
            Err(VfsError::Unavailable("tar entry limit exceeded".into()))
        } else {
            Ok(())
        }
    }
}

impl Default for TarIndex {
    fn default() -> Self {
        Self::empty()
    }
}

#[derive(Clone, Default)]
struct PaxValues {
    path: Option<Vec<u8>>,
    link_path: Option<Vec<u8>>,
    size: Option<u64>,
    uid: Option<u64>,
    gid: Option<u64>,
    mtime: Option<i64>,
}

impl PaxValues {
    fn overlay(&mut self, other: Self) {
        if other.path.is_some() {
            self.path = other.path;
        }
        if other.link_path.is_some() {
            self.link_path = other.link_path;
        }
        if other.size.is_some() {
            self.size = other.size;
        }
        if other.uid.is_some() {
            self.uid = other.uid;
        }
        if other.gid.is_some() {
            self.gid = other.gid;
        }
        if other.mtime.is_some() {
            self.mtime = other.mtime;
        }
    }
}

struct RawHeader {
    path: Vec<u8>,
    link_target: Option<Vec<u8>>,
    type_flag: u8,
    kind: TarEntryKind,
    size: u64,
    mode: u64,
    uid: u64,
    gid: u64,
    mtime: i64,
    device_major: Option<u64>,
    device_minor: Option<u64>,
}

pub(crate) struct TarStreamReader {
    stream: ArchiveByteStream,
    buffer: BytesMut,
    limits: TarLimits,
    received: u64,
    ended: bool,
    archive_finished: bool,
    current_size: Option<u64>,
    global_pax: PaxValues,
    pending_pax: PaxValues,
    pending_long_name: Option<Vec<u8>>,
    pending_long_link: Option<Vec<u8>>,
}

impl TarStreamReader {
    pub(crate) fn new(stream: ArchiveByteStream, limits: TarLimits) -> Self {
        Self {
            stream,
            buffer: BytesMut::new(),
            limits,
            received: 0,
            ended: false,
            archive_finished: false,
            current_size: None,
            global_pax: PaxValues::default(),
            pending_pax: PaxValues::default(),
            pending_long_name: None,
            pending_long_link: None,
        }
    }

    pub(crate) async fn next_entry(&mut self) -> Result<Option<TarEntry>, VfsError> {
        if self.current_size.is_some() {
            return Err(VfsError::InvalidInput("previous tar body was not consumed"));
        }
        if self.archive_finished {
            return Ok(None);
        }

        loop {
            let block = self.read_exact(TAR_BLOCK).await?;
            if block.iter().all(|byte| *byte == 0) {
                let second = self.read_exact(TAR_BLOCK).await?;
                if second.iter().any(|byte| *byte != 0) {
                    return Err(VfsError::Io("non-zero tar header after end marker".into()));
                }
                self.ensure_zero_tail().await?;
                self.archive_finished = true;
                return Ok(None);
            }

            let header = parse_header(&block)?;
            match header.type_flag {
                b'x' | b'g' => {
                    let payload = self.read_extension(header.size).await?;
                    let pax = parse_pax(&payload)?;
                    if header.type_flag == b'g' {
                        self.global_pax.overlay(pax);
                    } else {
                        self.pending_pax.overlay(pax);
                    }
                    continue;
                }
                b'L' | b'K' => {
                    let payload = self.read_extension(header.size).await?;
                    let value = parse_long_value(&payload)?;
                    if header.type_flag == b'L' {
                        self.pending_long_name = Some(value);
                    } else {
                        self.pending_long_link = Some(value);
                    }
                    continue;
                }
                _ => {}
            }

            let mut pax = self.global_pax.clone();
            pax.overlay(std::mem::take(&mut self.pending_pax));
            let raw_path = pax
                .path
                .take()
                .or_else(|| self.pending_long_name.take())
                .unwrap_or(header.path);
            let link_target = pax
                .link_path
                .take()
                .or_else(|| self.pending_long_link.take())
                .or(header.link_target);
            validate_link_target(link_target.as_deref(), &self.limits)?;
            if header.kind == TarEntryKind::Hardlink {
                TarPath::from_tar_bytes(
                    link_target
                        .as_deref()
                        .ok_or_else(|| VfsError::Io("hardlink has no target".into()))?,
                    &self.limits,
                )?;
            }
            let size = pax.size.unwrap_or(header.size);
            if size > self.limits.max_entry_bytes {
                return Err(VfsError::Unavailable(
                    "tar entry size limit exceeded".into(),
                ));
            }
            let entry = TarEntry {
                path: TarPath::from_tar_bytes(&raw_path, &self.limits)?,
                raw_path,
                kind: header.kind,
                size,
                mode: checked_u32(header.mode, "tar mode")?,
                uid: checked_u32(pax.uid.unwrap_or(header.uid), "tar uid")?,
                gid: checked_u32(pax.gid.unwrap_or(header.gid), "tar gid")?,
                mtime: pax.mtime.unwrap_or(header.mtime),
                link_target,
                device_major: header
                    .device_major
                    .map(|value| checked_u32(value, "device major"))
                    .transpose()?,
                device_minor: header
                    .device_minor
                    .map(|value| checked_u32(value, "device minor"))
                    .transpose()?,
                synthetic: false,
            };
            self.current_size = Some(size);
            return Ok(Some(entry));
        }
    }

    pub(crate) async fn read_body_chunk(
        &mut self,
        maximum: usize,
    ) -> Result<Option<Bytes>, VfsError> {
        let remaining = self
            .current_size
            .ok_or(VfsError::InvalidInput("no current tar body"))?;
        if remaining == 0 {
            return Ok(None);
        }
        self.fill().await?;
        if self.buffer.is_empty() {
            return Err(VfsError::Io("truncated tar archive".into()));
        }
        let take = maximum
            .max(1)
            .min(self.buffer.len())
            .min(remaining as usize);
        let chunk = self.buffer.split_to(take).freeze();
        self.current_size = Some(remaining - take as u64);
        Ok(Some(chunk))
    }

    pub(crate) async fn skip_entry_body(&mut self) -> Result<(), VfsError> {
        let size = self
            .current_size
            .take()
            .ok_or(VfsError::InvalidInput("no current tar body"))?;
        self.skip_exact(size).await?;
        self.skip_exact(padding(size, size)).await
    }

    pub(crate) async fn complete_consumed_body(
        &mut self,
        original_size: u64,
    ) -> Result<(), VfsError> {
        let remaining = self
            .current_size
            .take()
            .ok_or(VfsError::InvalidInput("no current tar body"))?;
        if remaining != 0 {
            return Err(VfsError::InvalidInput("tar body is incomplete"));
        }
        self.skip_exact(padding(original_size, original_size)).await
    }

    async fn read_extension(&mut self, size: u64) -> Result<Vec<u8>, VfsError> {
        if size > self.limits.max_extension_bytes as u64 {
            return Err(VfsError::Unavailable("tar extension limit exceeded".into()));
        }
        let data = self.read_exact(size as usize).await?;
        self.skip_exact(padding(size, size)).await?;
        Ok(data)
    }

    async fn fill(&mut self) -> Result<(), VfsError> {
        while self.buffer.is_empty() && !self.ended {
            match self.stream.next().await {
                Some(Ok(chunk)) if chunk.is_empty() => {}
                Some(Ok(chunk)) => {
                    self.received = self
                        .received
                        .checked_add(chunk.len() as u64)
                        .ok_or_else(|| VfsError::Unavailable("tar byte limit exceeded".into()))?;
                    if self.received > self.limits.max_archive_bytes {
                        return Err(VfsError::Unavailable("tar byte limit exceeded".into()));
                    }
                    self.buffer.extend_from_slice(&chunk);
                }
                Some(Err(error)) => return Err(error),
                None => self.ended = true,
            }
        }
        Ok(())
    }

    async fn read_exact(&mut self, length: usize) -> Result<Vec<u8>, VfsError> {
        let mut result = Vec::with_capacity(length);
        while result.len() < length {
            self.fill().await?;
            if self.buffer.is_empty() {
                return Err(VfsError::Io("truncated tar archive".into()));
            }
            let take = (length - result.len()).min(self.buffer.len());
            result.extend_from_slice(&self.buffer.split_to(take));
        }
        Ok(result)
    }

    async fn skip_exact(&mut self, mut length: u64) -> Result<(), VfsError> {
        while length != 0 {
            self.fill().await?;
            if self.buffer.is_empty() {
                return Err(VfsError::Io("truncated tar archive".into()));
            }
            let take = length.min(self.buffer.len() as u64) as usize;
            let _ = self.buffer.split_to(take);
            length -= take as u64;
        }
        Ok(())
    }

    async fn ensure_zero_tail(&mut self) -> Result<(), VfsError> {
        loop {
            self.fill().await?;
            if self.buffer.is_empty() {
                return Ok(());
            }
            if self.buffer.iter().any(|byte| *byte != 0) {
                return Err(VfsError::Io("non-zero bytes after tar end marker".into()));
            }
            self.buffer.clear();
        }
    }
}

fn padding(_remaining: u64, original_size: u64) -> u64 {
    (TAR_BLOCK as u64 - original_size % TAR_BLOCK as u64) % TAR_BLOCK as u64
}

fn parse_header(block: &[u8]) -> Result<RawHeader, VfsError> {
    if block.len() != TAR_BLOCK {
        return Err(VfsError::Io("truncated tar header".into()));
    }
    verify_checksum(block)?;
    let name = text_field(&block[0..100]);
    let prefix = text_field(&block[345..500]);
    let path = if prefix.is_empty() {
        name
    } else if name.is_empty() {
        prefix
    } else {
        [prefix.as_slice(), b"/", name.as_slice()].concat()
    };
    if path.is_empty() {
        return Err(VfsError::Io("empty tar entry path".into()));
    }
    let type_flag = block[156];
    let kind = match type_flag {
        0 | b'0' | b'7' => TarEntryKind::RegularFile,
        b'5' => TarEntryKind::Directory,
        b'2' => TarEntryKind::Symlink,
        b'1' => TarEntryKind::Hardlink,
        b'3' => TarEntryKind::CharacterDevice,
        b'4' => TarEntryKind::BlockDevice,
        b'6' => TarEntryKind::NamedPipe,
        _ => TarEntryKind::Other,
    };
    let link = text_field(&block[157..257]);
    Ok(RawHeader {
        path,
        link_target: (!link.is_empty()).then_some(link),
        type_flag,
        kind,
        size: tar_number(&block[124..136], "size")?,
        mode: tar_number(&block[100..108], "mode")?,
        uid: tar_number(&block[108..116], "uid")?,
        gid: tar_number(&block[116..124], "gid")?,
        mtime: tar_number(&block[136..148], "mtime")?
            .try_into()
            .map_err(|_| VfsError::Io("tar mtime overflow".into()))?,
        device_major: optional_tar_number(&block[329..337], "device major")?,
        device_minor: optional_tar_number(&block[337..345], "device minor")?,
    })
}

fn text_field(field: &[u8]) -> Vec<u8> {
    let end = field
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(field.len());
    field[..end].to_vec()
}

fn optional_tar_number(field: &[u8], label: &str) -> Result<Option<u64>, VfsError> {
    if field.iter().all(|byte| matches!(*byte, 0 | b' ')) {
        Ok(None)
    } else {
        tar_number(field, label).map(Some)
    }
}

fn tar_number(field: &[u8], label: &str) -> Result<u64, VfsError> {
    if field.is_empty() {
        return Err(VfsError::Io(format!("empty tar {label}")));
    }
    if field[0] & 0x80 != 0 {
        if field[0] & 0x40 != 0 {
            return Err(VfsError::Io(format!("negative tar {label}")));
        }
        let mut value = u64::from(field[0] & 0x3f);
        for byte in &field[1..] {
            value = value
                .checked_mul(256)
                .and_then(|value| value.checked_add(u64::from(*byte)))
                .ok_or_else(|| VfsError::Io(format!("tar {label} overflow")))?;
        }
        return Ok(value);
    }
    let value = field
        .iter()
        .copied()
        .skip_while(|byte| matches!(*byte, 0 | b' '))
        .take_while(|byte| !matches!(*byte, 0 | b' '))
        .collect::<Vec<_>>();
    if value.is_empty() {
        return Ok(0);
    }
    if !value.iter().all(|byte| matches!(*byte, b'0'..=b'7')) {
        return Err(VfsError::Io(format!("invalid octal tar {label}")));
    }
    let text = std::str::from_utf8(&value).map_err(|_| VfsError::Io(format!("tar {label}")))?;
    u64::from_str_radix(text, 8).map_err(|_| VfsError::Io(format!("tar {label} overflow")))
}

fn verify_checksum(block: &[u8]) -> Result<(), VfsError> {
    let expected = tar_number(&block[148..156], "checksum")?;
    let actual = block
        .iter()
        .enumerate()
        .map(|(index, byte)| {
            if (148..156).contains(&index) {
                u64::from(b' ')
            } else {
                u64::from(*byte)
            }
        })
        .sum::<u64>();
    if actual != expected {
        return Err(VfsError::Io("tar checksum mismatch".into()));
    }
    Ok(())
}

fn parse_pax(data: &[u8]) -> Result<PaxValues, VfsError> {
    let mut result = PaxValues::default();
    let mut offset = 0;
    while offset < data.len() {
        let space = data[offset..]
            .iter()
            .position(|byte| *byte == b' ')
            .map(|position| position + offset)
            .ok_or_else(|| VfsError::Io("PAX record has no length".into()))?;
        let length = std::str::from_utf8(&data[offset..space])
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .ok_or_else(|| VfsError::Io("invalid PAX record length".into()))?;
        let end = offset
            .checked_add(length)
            .ok_or_else(|| VfsError::Io("PAX record overflow".into()))?;
        if end > data.len() || end <= space + 2 || data[end - 1] != b'\n' {
            return Err(VfsError::Io("invalid PAX record boundary".into()));
        }
        let record = &data[space + 1..end - 1];
        let equals = record
            .iter()
            .position(|byte| *byte == b'=')
            .ok_or_else(|| VfsError::Io("invalid PAX key/value".into()))?;
        let key = &record[..equals];
        let value = &record[equals + 1..];
        if value.contains(&0) {
            return Err(VfsError::Io("NUL in PAX value".into()));
        }
        match key {
            b"path" => result.path = Some(value.to_vec()),
            b"linkpath" => result.link_path = Some(value.to_vec()),
            b"size" => result.size = Some(decimal_u64(value, "PAX size")?),
            b"uid" => result.uid = Some(decimal_u64(value, "PAX uid")?),
            b"gid" => result.gid = Some(decimal_u64(value, "PAX gid")?),
            b"mtime" => result.mtime = Some(pax_mtime(value)?),
            _ => {}
        }
        offset = end;
    }
    Ok(result)
}

fn decimal_u64(value: &[u8], label: &str) -> Result<u64, VfsError> {
    if value.is_empty() || !value.iter().all(u8::is_ascii_digit) {
        return Err(VfsError::Io(format!("invalid {label}")));
    }
    std::str::from_utf8(value)
        .ok()
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| VfsError::Io(format!("{label} overflow")))
}

fn pax_mtime(value: &[u8]) -> Result<i64, VfsError> {
    let integer = value.split(|byte| *byte == b'.').next().unwrap_or(value);
    std::str::from_utf8(integer)
        .ok()
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| VfsError::Io("invalid PAX mtime".into()))
}

fn parse_long_value(data: &[u8]) -> Result<Vec<u8>, VfsError> {
    let end = data
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(data.len());
    if data[end..].iter().any(|byte| !matches!(*byte, 0 | b'\n')) {
        return Err(VfsError::Io("invalid GNU long-name tail".into()));
    }
    let mut value = data[..end].to_vec();
    while value.last() == Some(&b'\n') {
        value.pop();
    }
    Ok(value)
}

fn validate_link_target(target: Option<&[u8]>, limits: &TarLimits) -> Result<(), VfsError> {
    if let Some(target) = target {
        if target.len() > limits.max_path_bytes {
            return Err(VfsError::PathTooLong);
        }
        if target.contains(&0) {
            return Err(VfsError::InvalidInput("NUL in tar link target"));
        }
        VirtualPathBytes::new(target)?;
    }
    Ok(())
}

fn checked_u32(value: u64, label: &str) -> Result<u32, VfsError> {
    value
        .try_into()
        .map_err(|_| VfsError::Io(format!("{label} overflow")))
}

#[cfg(test)]
mod tests {
    use futures_util::stream;

    use super::*;

    fn header(name: &[u8], kind: u8, size: usize, link: &[u8]) -> [u8; TAR_BLOCK] {
        let mut block = [0u8; TAR_BLOCK];
        block[..name.len()].copy_from_slice(name);
        write_octal(&mut block[100..108], 0o644);
        write_octal(&mut block[108..116], 12);
        write_octal(&mut block[116..124], 34);
        write_octal(&mut block[124..136], size as u64);
        write_octal(&mut block[136..148], 7);
        block[148..156].fill(b' ');
        block[156] = kind;
        block[157..157 + link.len()].copy_from_slice(link);
        block[257..263].copy_from_slice(b"ustar\0");
        block[263..265].copy_from_slice(b"00");
        let checksum: u64 = block.iter().map(|byte| u64::from(*byte)).sum();
        write_octal(&mut block[148..156], checksum);
        block
    }

    fn write_octal(field: &mut [u8], value: u64) {
        let text = format!("{:0width$o}", value, width = field.len() - 1);
        field[..text.len()].copy_from_slice(text.as_bytes());
        field[text.len()] = 0;
    }

    fn archive(entries: Vec<([u8; TAR_BLOCK], Vec<u8>)>) -> Vec<u8> {
        let mut tar = Vec::new();
        for (header, body) in entries {
            tar.extend_from_slice(&header);
            tar.extend_from_slice(&body);
            tar.resize(
                tar.len() + (TAR_BLOCK - body.len() % TAR_BLOCK) % TAR_BLOCK,
                0,
            );
        }
        tar.resize(tar.len() + TAR_BLOCK * 2, 0);
        tar
    }

    async fn parse(bytes: Vec<u8>) -> Result<TarIndex, VfsError> {
        TarIndex::from_stream(
            Box::pin(stream::iter([Ok(Bytes::from(bytes))])),
            TarLimits::default(),
        )
        .await
    }

    fn pax_path(value: &[u8]) -> Vec<u8> {
        let mut length = value.len() + b" path=\n".len() + 1;
        loop {
            let record = [length.to_string().as_bytes(), b" path=", value, b"\n"].concat();
            if record.len() == length {
                return record;
            }
            length = record.len();
        }
    }

    #[tokio::test]
    async fn indexes_raw_non_utf8_names_without_file_bodies() {
        let bytes = archive(vec![(
            header(b"dir/na\xffme", b'0', 5, b""),
            b"hello".to_vec(),
        )]);
        let index = parse(bytes).await.expect("index");
        let path = TarPath::from_tar_bytes(b"dir/na\xffme", &TarLimits::default()).expect("path");
        assert_eq!(index.get(&path).expect("entry").raw_path, b"dir/na\xffme");
        assert!(
            index
                .get(&TarPath::from_tar_bytes(b"dir", &TarLimits::default()).unwrap())
                .unwrap()
                .synthetic
        );
    }

    #[tokio::test]
    async fn expands_pax_and_gnu_long_names_as_raw_bytes() {
        let pax_name = b"pax/na\xffme";
        let pax = pax_path(pax_name);
        let gnu_name = [b"gnu/".as_slice(), vec![b'x'; 120].as_slice()].concat();
        let mut gnu_payload = gnu_name.clone();
        gnu_payload.push(0);
        let bytes = archive(vec![
            (header(b"PaxHeader", b'x', pax.len(), b""), pax),
            (header(b"ignored-pax", b'0', 0, b""), vec![]),
            (
                header(b"LongName", b'L', gnu_payload.len(), b""),
                gnu_payload,
            ),
            (header(b"ignored-gnu", b'0', 0, b""), vec![]),
        ]);
        let index = parse(bytes).await.expect("index");
        assert!(
            index
                .get(&TarPath::from_tar_bytes(pax_name, &TarLimits::default()).unwrap())
                .is_some()
        );
        assert!(
            index
                .get(&TarPath::from_tar_bytes(&gnu_name, &TarLimits::default()).unwrap())
                .is_some()
        );
    }

    #[tokio::test]
    async fn rejects_component_traversal_and_archive_limit() {
        let bytes = archive(vec![(header(b"ok/../escape", b'0', 0, b""), vec![])]);
        assert_eq!(parse(bytes).await.unwrap_err(), VfsError::SymlinkEscape);

        let bytes = archive(vec![(header(b"large", b'0', 700, b""), vec![0; 700])]);
        let limits = TarLimits {
            max_archive_bytes: 1024,
            max_entry_bytes: 700,
            ..TarLimits::default()
        };
        let error = TarIndex::from_stream(Box::pin(stream::iter([Ok(Bytes::from(bytes))])), limits)
            .await
            .unwrap_err();
        assert!(matches!(error, VfsError::Unavailable(_)));
    }

    #[tokio::test]
    async fn preserves_absolute_symlink_as_metadata() {
        let bytes = archive(vec![(
            header(b"lib/link", b'2', 0, b"/host/must-not-resolve"),
            vec![],
        )]);
        let index = parse(bytes).await.expect("index");
        let entry = index
            .get(&TarPath::from_tar_bytes(b"lib/link", &TarLimits::default()).unwrap())
            .unwrap();
        assert_eq!(
            entry.link_target.as_deref(),
            Some(b"/host/must-not-resolve".as_slice())
        );
    }
}
