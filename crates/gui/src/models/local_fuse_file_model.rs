//! Qt-free row mapping for the unified local FUSE file model.

use chrono::{DateTime, Utc};

use crate::controllers::local_fuse_files::{LocalFileEntry, LocalFileKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalFuseFileRow {
    pub name: String,
    pub path_token: String,
    pub display_path: String,
    pub entry_type: String,
    pub icon_name: String,
    pub size_bytes: i64,
    pub size_text: String,
    pub modified_text: String,
    pub kind_text: String,
    pub hidden: bool,
    pub readable: bool,
    pub symlink_target: String,
    pub mode_text: String,
    pub owner_text: String,
}

pub fn map_local_fuse_row(entry: &LocalFileEntry) -> LocalFuseFileRow {
    LocalFuseFileRow {
        name: entry.display_name.clone(),
        path_token: entry.path_token.clone(),
        display_path: entry.display_path.clone(),
        entry_type: entry.kind.as_str().into(),
        icon_name: icon_name(entry).into(),
        size_bytes: entry.size.min(i64::MAX as u64) as i64,
        size_text: if entry.kind.is_directory() {
            "—".into()
        } else {
            format_bytes(entry.size)
        },
        modified_text: format_modified(entry.modified_unix_seconds),
        kind_text: kind_text(entry),
        hidden: entry.hidden,
        readable: entry.kind.is_previewable() || entry.kind.is_directory(),
        symlink_target: entry.symlink_target_display.clone(),
        mode_text: format!(
            "{:04o} ({})",
            entry.mode & 0o7777,
            symbolic_mode(entry.mode)
        ),
        owner_text: format!("{}:{}", entry.uid, entry.gid),
    }
}

fn icon_name(entry: &LocalFileEntry) -> &'static str {
    match entry.kind {
        LocalFileKind::Directory => "folder",
        LocalFileKind::Symlink => "emblem-symbolic-link",
        LocalFileKind::Socket => "network-server",
        LocalFileKind::Fifo => "inode-fifo",
        LocalFileKind::BlockDevice | LocalFileKind::CharacterDevice => "drive-harddisk",
        LocalFileKind::Unknown => "unknown",
        LocalFileKind::RegularFile => icon_for_name(&entry.display_name),
    }
}

fn icon_for_name(name: &str) -> &'static str {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".json") {
        "application-json"
    } else if lower.ends_with(".png")
        || lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".gif")
        || lower.ends_with(".webp")
        || lower.ends_with(".svg")
    {
        "image-x-generic"
    } else if lower.ends_with(".zip")
        || lower.ends_with(".tar")
        || lower.ends_with(".gz")
        || lower.ends_with(".xz")
        || lower.ends_with(".7z")
    {
        "package-x-generic"
    } else if lower.ends_with(".sh") || lower.ends_with(".bin") {
        "application-x-executable"
    } else {
        "text-x-generic"
    }
}

fn kind_text(entry: &LocalFileEntry) -> String {
    match entry.kind {
        LocalFileKind::Directory => "Folder".into(),
        LocalFileKind::RegularFile => file_kind_text(&entry.display_name),
        LocalFileKind::Symlink if !entry.symlink_target_display.is_empty() => {
            format!("Symbolic Link → {}", entry.symlink_target_display)
        }
        LocalFileKind::Symlink => "Symbolic Link".into(),
        LocalFileKind::Socket => "Socket".into(),
        LocalFileKind::Fifo => "FIFO".into(),
        LocalFileKind::BlockDevice => "Block Device".into(),
        LocalFileKind::CharacterDevice => "Character Device".into(),
        LocalFileKind::Unknown => "Unknown".into(),
    }
}

fn file_kind_text(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".json") {
        "JSON Document".into()
    } else if lower.ends_with(".png") {
        "PNG Image".into()
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "JPEG Image".into()
    } else if lower.ends_with(".gif") {
        "GIF Image".into()
    } else if lower.ends_with(".svg") {
        "SVG Image".into()
    } else if lower.ends_with(".txt") || lower.ends_with(".log") || lower.ends_with(".md") {
        "Text Document".into()
    } else {
        "File".into()
    }
}

pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

fn format_modified(unix_seconds: i64) -> String {
    DateTime::<Utc>::from_timestamp(unix_seconds, 0)
        .map(|date| date.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| "Unknown".into())
}

fn symbolic_mode(mode: u32) -> String {
    let mut output = String::with_capacity(9);
    for shift in [6, 3, 0] {
        let bits = (mode >> shift) & 0b111;
        output.push(if bits & 0b100 != 0 { 'r' } else { '-' });
        output.push(if bits & 0b010 != 0 { 'w' } else { '-' });
        output.push(if bits & 0b001 != 0 { 'x' } else { '-' });
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(kind: LocalFileKind, name: &str) -> LocalFileEntry {
        LocalFileEntry {
            name_raw: name.as_bytes().to_vec(),
            display_name: name.into(),
            path_components: vec![name.as_bytes().to_vec()],
            path_token: "/%66".into(),
            display_path: format!("/{name}"),
            kind,
            size: 1536,
            modified_unix_seconds: 1_700_000_000,
            mode: 0o100444,
            uid: 1000,
            gid: 1000,
            hidden: false,
            symlink_target_raw: None,
            symlink_target_display: String::new(),
        }
    }

    #[test]
    fn fixture_maps_read_only_metadata_and_separate_size_kind_columns() {
        let row = map_local_fuse_row(&fixture(LocalFileKind::RegularFile, "data.json"));
        assert_eq!(row.icon_name, "application-json");
        assert_eq!(row.size_text, "1.5 KiB");
        assert_eq!(row.kind_text, "JSON Document");
        assert_eq!(row.mode_text, "0444 (r--r--r--)");
        assert_eq!(row.owner_text, "1000:1000");
    }

    #[test]
    fn special_nodes_are_not_advertised_as_readable() {
        let row = map_local_fuse_row(&fixture(LocalFileKind::CharacterDevice, "tty"));
        assert!(!row.readable);
        assert_eq!(row.kind_text, "Character Device");
    }
}
