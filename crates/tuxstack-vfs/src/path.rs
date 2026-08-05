use std::borrow::Cow;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::sync::Arc;

use crate::VfsError;

pub const MAX_NAME_BYTES: usize = 255;
pub const DEFAULT_MAX_PATH_BYTES: usize = 16 * 1024;

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct VirtualFileName(Arc<[u8]>);

impl VirtualFileName {
    pub fn new(bytes: impl AsRef<[u8]>) -> Result<Self, VfsError> {
        let bytes = bytes.as_ref();
        if bytes.is_empty() {
            return Err(VfsError::InvalidInput("an empty name is not a component"));
        }
        if bytes.contains(&0) {
            return Err(VfsError::InvalidInput("NUL is not allowed in file names"));
        }
        if bytes == b"." || bytes == b".." {
            return Err(VfsError::InvalidInput("dot components are not file names"));
        }
        if bytes.contains(&b'/') {
            return Err(VfsError::InvalidInput(
                "slash is not allowed in a component",
            ));
        }
        if bytes.len() > MAX_NAME_BYTES {
            return Err(VfsError::NameTooLong);
        }
        Ok(Self(Arc::from(bytes)))
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn as_os_str(&self) -> &OsStr {
        OsStr::from_bytes(self.as_bytes())
    }
}

impl fmt::Debug for VirtualFileName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "VirtualFileName({:?})", self.as_os_str())
    }
}

impl TryFrom<&OsStr> for VirtualFileName {
    type Error = VfsError;

    fn try_from(value: &OsStr) -> Result<Self, Self::Error> {
        Self::new(value.as_bytes())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct VirtualPath {
    components: Arc<[VirtualFileName]>,
    byte_len: usize,
}

impl VirtualPath {
    pub fn root() -> Self {
        Self {
            components: Arc::from([]),
            byte_len: 1,
        }
    }

    pub fn from_absolute(bytes: impl AsRef<[u8]>) -> Result<Self, VfsError> {
        Self::from_absolute_with_limit(bytes, DEFAULT_MAX_PATH_BYTES)
    }

    pub fn from_absolute_with_limit(
        bytes: impl AsRef<[u8]>,
        max_bytes: usize,
    ) -> Result<Self, VfsError> {
        let bytes = bytes.as_ref();
        if bytes.contains(&0) {
            return Err(VfsError::InvalidInput("NUL is not allowed in paths"));
        }
        if bytes.first() != Some(&b'/') {
            return Err(VfsError::InvalidInput("VirtualPath must be absolute"));
        }
        if bytes.len() > max_bytes {
            return Err(VfsError::PathTooLong);
        }
        let mut path = Self::root();
        for raw in bytes.split(|byte| *byte == b'/').skip(1) {
            if raw.is_empty() || raw == b"." {
                continue;
            }
            if raw == b".." {
                path = path.parent().ok_or(VfsError::SymlinkEscape)?;
                continue;
            }
            path = path.join(&VirtualFileName::new(raw)?)?;
        }
        Ok(path)
    }

    pub fn from_components(
        components: impl IntoIterator<Item = VirtualFileName>,
    ) -> Result<Self, VfsError> {
        let components: Vec<_> = components.into_iter().collect();
        let is_empty = components.is_empty();
        let byte_len = 1 + components
            .iter()
            .map(|item| item.as_bytes().len() + 1)
            .sum::<usize>();
        if byte_len.saturating_sub(usize::from(!is_empty)) > DEFAULT_MAX_PATH_BYTES {
            return Err(VfsError::PathTooLong);
        }
        Ok(Self {
            components: components.into(),
            byte_len: byte_len.saturating_sub(usize::from(!is_empty)),
        })
    }

    pub fn components(&self) -> &[VirtualFileName] {
        &self.components
    }

    pub fn is_root(&self) -> bool {
        self.components.is_empty()
    }

    pub fn depth(&self) -> usize {
        self.components.len()
    }

    pub fn byte_len(&self) -> usize {
        self.byte_len.max(1)
    }

    pub fn file_name(&self) -> Option<&VirtualFileName> {
        self.components.last()
    }

    pub fn parent(&self) -> Option<Self> {
        (!self.is_root()).then(|| {
            Self::from_components(self.components[..self.depth() - 1].iter().cloned())
                .expect("existing path is valid")
        })
    }

    pub fn join(&self, name: &VirtualFileName) -> Result<Self, VfsError> {
        let mut components = self.components.to_vec();
        components.push(name.clone());
        Self::from_components(components)
    }

    pub fn starts_with(&self, prefix: &Self) -> bool {
        self.components.starts_with(&prefix.components)
    }

    pub fn strip_prefix(&self, prefix: &Self) -> Option<Self> {
        self.starts_with(prefix).then(|| {
            Self::from_components(self.components[prefix.depth()..].iter().cloned())
                .expect("subpath is valid")
        })
    }

    pub fn as_bytes(&self) -> Vec<u8> {
        if self.is_root() {
            return vec![b'/'];
        }
        let mut result = Vec::with_capacity(self.byte_len());
        for component in self.components() {
            result.push(b'/');
            result.extend_from_slice(component.as_bytes());
        }
        result
    }
}

impl Default for VirtualPath {
    fn default() -> Self {
        Self::root()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct VirtualPathBytes(Arc<[u8]>);

impl VirtualPathBytes {
    pub fn new(bytes: impl AsRef<[u8]>) -> Result<Self, VfsError> {
        let bytes = bytes.as_ref();
        if bytes.contains(&0) {
            return Err(VfsError::InvalidInput("NUL is not allowed in link targets"));
        }
        if bytes.len() > DEFAULT_MAX_PATH_BYTES {
            return Err(VfsError::PathTooLong);
        }
        Ok(Self(Arc::from(bytes)))
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn is_absolute(&self) -> bool {
        self.0.first() == Some(&b'/')
    }

    pub fn to_os_string(&self) -> OsString {
        OsString::from_vec(self.as_bytes().to_vec())
    }
}

pub struct FuseNameCodec;

impl FuseNameCodec {
    pub fn encode(name: &[u8]) -> Result<String, VfsError> {
        if name.contains(&0) {
            return Err(VfsError::InvalidInput("NUL is not encodable"));
        }
        if name == b"." {
            return Ok("%2E".to_owned());
        }
        if name == b".." {
            return Ok("%2E%2E".to_owned());
        }

        let mut encoded = String::with_capacity(name.len());
        let mut cursor = 0;
        while cursor < name.len() {
            if let Ok(text) = std::str::from_utf8(&name[cursor..]) {
                encode_unicode(text, &mut encoded);
                break;
            }
            let error = std::str::from_utf8(&name[cursor..]).expect_err("invalid UTF-8 expected");
            let valid_end = cursor + error.valid_up_to();
            if valid_end > cursor {
                let valid = std::str::from_utf8(&name[cursor..valid_end]).expect("validated UTF-8");
                encode_unicode(valid, &mut encoded);
            }
            let invalid_len = error.error_len().unwrap_or(name.len() - valid_end);
            for byte in &name[valid_end..valid_end + invalid_len] {
                push_percent(*byte, &mut encoded);
            }
            cursor = valid_end + invalid_len;
        }
        Ok(encoded)
    }

    pub fn decode(encoded: &str) -> Result<Vec<u8>, VfsError> {
        let mut result = Vec::with_capacity(encoded.len());
        let bytes = encoded.as_bytes();
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index] == b'%' {
                if index + 2 >= bytes.len() {
                    return Err(VfsError::InvalidEncoding);
                }
                let high = hex(bytes[index + 1]).ok_or(VfsError::InvalidEncoding)?;
                let low = hex(bytes[index + 2]).ok_or(VfsError::InvalidEncoding)?;
                let byte = high << 4 | low;
                if byte == 0 {
                    return Err(VfsError::InvalidInput("decoded NUL is not allowed"));
                }
                result.push(byte);
                index += 3;
            } else {
                let character = encoded[index..]
                    .chars()
                    .next()
                    .ok_or(VfsError::InvalidEncoding)?;
                let mut buffer = [0; 4];
                result.extend_from_slice(character.encode_utf8(&mut buffer).as_bytes());
                index += character.len_utf8();
            }
        }
        Ok(result)
    }

    /// Deterministically disambiguates a friendly name while preserving reversibility
    /// of the encoded-name portion. `stable_id` should be the canonical Docker identity.
    pub fn with_collision_suffix(encoded: &str, stable_id: &str) -> String {
        let hash = fnv1a64(stable_id.as_bytes());
        format!("{encoded}~{hash:016x}")
    }

    pub fn split_collision_suffix(name: &str) -> Cow<'_, str> {
        match name.rsplit_once('~') {
            Some((base, suffix))
                if suffix.len() == 16 && suffix.bytes().all(|byte| byte.is_ascii_hexdigit()) =>
            {
                Cow::Borrowed(base)
            }
            _ => Cow::Borrowed(name),
        }
    }
}

fn encode_unicode(text: &str, output: &mut String) {
    for character in text.chars() {
        if is_safe(character) {
            output.push(character);
        } else {
            let mut buffer = [0; 4];
            for byte in character.encode_utf8(&mut buffer).as_bytes() {
                push_percent(*byte, output);
            }
        }
    }
}

fn is_safe(character: char) -> bool {
    !character.is_control() && !matches!(character, '/' | ':' | '@' | '%' | '\\')
}

fn push_percent(byte: u8, output: &mut String) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    output.push('%');
    output.push(HEX[(byte >> 4) as usize] as char);
    output.push(HEX[(byte & 0x0f) as usize] as char);
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}
