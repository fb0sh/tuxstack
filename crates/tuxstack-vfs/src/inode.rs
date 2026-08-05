use std::collections::{HashMap, HashSet};

use crate::{VfsError, VirtualPath};

pub const ROOT_INODE: u64 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct VirtualNodeKey {
    pub daemon_identity: String,
    pub resource_kind: String,
    pub resource_id: String,
    pub provider_key: String,
    pub logical_path: VirtualPath,
    pub generation: u64,
}

impl VirtualNodeKey {
    pub fn root() -> Self {
        Self {
            daemon_identity: String::new(),
            resource_kind: "root".to_owned(),
            resource_id: String::new(),
            provider_key: "namespace".to_owned(),
            logical_path: VirtualPath::root(),
            generation: 0,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct HardlinkKey {
    pub daemon_identity: String,
    pub resource_kind: String,
    pub resource_id: String,
    pub provider_key: String,
    pub provider_node_id: Vec<u8>,
    pub generation: u64,
}

#[derive(Clone, Debug)]
pub struct InodeRecord {
    pub inode: u64,
    pub canonical_key: VirtualNodeKey,
    pub hardlink_key: Option<HardlinkKey>,
    pub aliases: HashSet<VirtualNodeKey>,
    pub deleted: bool,
    pub lookup_count: u64,
}

#[derive(Debug)]
pub struct InodeTable {
    by_key: HashMap<VirtualNodeKey, u64>,
    by_hardlink: HashMap<HardlinkKey, u64>,
    by_inode: HashMap<u64, InodeRecord>,
    next_inode: u64,
}

impl InodeTable {
    pub fn new() -> Self {
        let root_key = VirtualNodeKey::root();
        let root_record = InodeRecord {
            inode: ROOT_INODE,
            canonical_key: root_key.clone(),
            hardlink_key: None,
            aliases: HashSet::from([root_key.clone()]),
            deleted: false,
            lookup_count: 1,
        };
        Self {
            by_key: HashMap::from([(root_key, ROOT_INODE)]),
            by_hardlink: HashMap::new(),
            by_inode: HashMap::from([(ROOT_INODE, root_record)]),
            next_inode: ROOT_INODE + 1,
        }
    }

    pub fn inode_for(
        &mut self,
        key: VirtualNodeKey,
        hardlink_key: Option<HardlinkKey>,
    ) -> Result<u64, VfsError> {
        if let Some(inode) = self.by_key.get(&key).copied() {
            if let Some(record) = self.by_inode.get_mut(&inode) {
                if record.deleted {
                    return Err(VfsError::NotFound);
                }
                record.lookup_count = record.lookup_count.saturating_add(1);
            }
            return Ok(inode);
        }

        if let Some((hardlink, inode)) = hardlink_key.as_ref().and_then(|hardlink| {
            self.by_hardlink
                .get(hardlink)
                .map(|inode| (hardlink, *inode))
        }) {
            let record = self.by_inode.get_mut(&inode).ok_or(VfsError::Stale)?;
            if !record.deleted {
                record.aliases.insert(key.clone());
                record.lookup_count = record.lookup_count.saturating_add(1);
                self.by_key.insert(key, inode);
                debug_assert_eq!(record.hardlink_key.as_ref(), Some(hardlink));
                return Ok(inode);
            }
        }

        let inode = self.next_inode;
        self.next_inode = self.next_inode.checked_add(1).ok_or(VfsError::Unavailable(
            "inode number space exhausted".to_owned(),
        ))?;
        let record = InodeRecord {
            inode,
            canonical_key: key.clone(),
            hardlink_key: hardlink_key.clone(),
            aliases: HashSet::from([key.clone()]),
            deleted: false,
            lookup_count: 1,
        };
        self.by_key.insert(key, inode);
        if let Some(hardlink_key) = hardlink_key {
            self.by_hardlink.insert(hardlink_key, inode);
        }
        self.by_inode.insert(inode, record);
        Ok(inode)
    }

    pub fn add_alias(&mut self, inode: u64, alias: VirtualNodeKey) -> Result<(), VfsError> {
        let record = self.by_inode.get_mut(&inode).ok_or(VfsError::NotFound)?;
        if record.deleted {
            return Err(VfsError::NotFound);
        }
        if let Some(existing) = self.by_key.get(&alias) {
            return (*existing == inode)
                .then_some(())
                .ok_or(VfsError::InvalidInput(
                    "alias already identifies another inode",
                ));
        }
        record.aliases.insert(alias.clone());
        self.by_key.insert(alias, inode);
        Ok(())
    }

    pub fn rename_alias(
        &mut self,
        old: &VirtualNodeKey,
        new: VirtualNodeKey,
    ) -> Result<u64, VfsError> {
        if self.by_key.contains_key(&new) {
            return Err(VfsError::InvalidInput("rename destination already exists"));
        }
        let inode = self.by_key.remove(old).ok_or(VfsError::NotFound)?;
        let record = self.by_inode.get_mut(&inode).ok_or(VfsError::Stale)?;
        record.aliases.remove(old);
        record.aliases.insert(new.clone());
        if &record.canonical_key == old {
            record.canonical_key = new.clone();
        }
        self.by_key.insert(new, inode);
        Ok(inode)
    }

    /// Removes namespace aliases but retains the inode record forever. Inodes are never
    /// recycled during a daemon lifetime, so stale kernel cache references cannot collide
    /// with an unrelated future node. Existing open handles remain independently valid.
    pub fn delete(&mut self, inode: u64) -> Result<(), VfsError> {
        if inode == ROOT_INODE {
            return Err(VfsError::InvalidInput("root cannot be deleted"));
        }
        let record = self.by_inode.get_mut(&inode).ok_or(VfsError::NotFound)?;
        record.deleted = true;
        for alias in record.aliases.drain() {
            self.by_key.remove(&alias);
        }
        if let Some(hardlink_key) = &record.hardlink_key {
            self.by_hardlink.remove(hardlink_key);
        }
        Ok(())
    }

    pub fn forget(&mut self, inode: u64, count: u64) {
        if let Some(record) = self.by_inode.get_mut(&inode) {
            record.lookup_count = record.lookup_count.saturating_sub(count);
        }
    }

    pub fn get(&self, inode: u64) -> Option<&InodeRecord> {
        self.by_inode.get(&inode)
    }

    pub fn lookup(&self, key: &VirtualNodeKey) -> Option<u64> {
        self.by_key.get(key).copied()
    }

    pub fn next_inode(&self) -> u64 {
        self.next_inode
    }
}

impl Default for InodeTable {
    fn default() -> Self {
        Self::new()
    }
}
