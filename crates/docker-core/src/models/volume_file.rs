//! Validated logical path type for volume and image file browsing.

/// Validated logical path inside a volume (`/` root, no `..`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct VolumePath {
    components: Vec<String>,
}

impl VolumePath {
    pub fn root() -> Self {
        Self {
            components: Vec::new(),
        }
    }

    /// Parse a logical volume path such as `/`, `/dir`, or `dir/sub`.
    pub fn parse(input: &str) -> Result<Self, String> {
        if input.contains('\0') {
            return Err("path contains a NUL byte".into());
        }
        let trimmed = input.trim();
        if trimmed.is_empty() || trimmed == "/" {
            return Ok(Self::root());
        }
        let without_root = trimmed.trim_start_matches('/');
        let mut components = Vec::new();
        for part in without_root.split('/') {
            if part.is_empty() || part == "." {
                continue;
            }
            if part == ".." {
                return Err("path must not contain '..'".into());
            }
            if part.contains('\0') {
                return Err("path component contains a NUL byte".into());
            }
            components.push(part.to_string());
        }
        Ok(Self { components })
    }

    pub fn components(&self) -> &[String] {
        &self.components
    }

    pub fn is_root(&self) -> bool {
        self.components.is_empty()
    }

    pub fn join_name(&self, name: &str) -> Result<Self, String> {
        if name.is_empty() || name == "." {
            return Ok(self.clone());
        }
        if name == ".." || name.contains('/') || name.contains('\0') {
            return Err("invalid path component".into());
        }
        let mut child = self.clone();
        child.components.push(name.to_string());
        Ok(child)
    }

    pub fn parent(&self) -> Option<Self> {
        if self.components.is_empty() {
            None
        } else {
            let mut parent = self.clone();
            parent.components.pop();
            Some(parent)
        }
    }

    /// Logical display path always starting with `/`.
    pub fn display(&self) -> String {
        if self.components.is_empty() {
            "/".into()
        } else {
            format!("/{}", self.components.join("/"))
        }
    }

    /// Absolute helper path under `/volume`.
    pub fn helper_absolute(&self) -> String {
        if self.components.is_empty() {
            "/volume".into()
        } else {
            format!("/volume/{}", self.components.join("/"))
        }
    }

    /// True when `other` is this path or a descendant of it.
    pub fn contains_path(&self, other: &Self) -> bool {
        other.components.starts_with(&self.components)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_rejects_parent_and_nul() {
        assert!(VolumePath::parse("../etc").is_err());
        assert!(VolumePath::parse("/a/\0/b").is_err());
        assert!(VolumePath::parse("/a/../b").is_err());
    }

    #[test]
    fn path_normalizes_dots_and_slashes() {
        let path = VolumePath::parse("///a//./b///").unwrap();
        assert_eq!(path.display(), "/a/b");
        assert_eq!(path.helper_absolute(), "/volume/a/b");
        assert_eq!(path.parent().unwrap().display(), "/a");
        assert!(VolumePath::root().parent().is_none());
    }

    #[test]
    fn join_name_rejects_traversal() {
        let root = VolumePath::root();
        assert!(root.join_name("..").is_err());
        assert!(root.join_name("a/b").is_err());
        assert_eq!(root.join_name("ok").unwrap().display(), "/ok");
    }
}
