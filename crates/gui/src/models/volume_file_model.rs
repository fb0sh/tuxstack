//! View mapping helpers for volume file rows and previews.

use chrono::{DateTime, Utc};
use tuxstack_docker_core::{FilesystemEntry, FilesystemEntryType, format};

#[derive(Debug, Clone)]
pub struct VolumeFileRow {
    pub name: String,
    pub display_name: String,
    pub path: String,
    pub entry_type: String,
    pub icon_name: String,
    pub size_bytes: i64,
    pub size_known: bool,
    pub size_text: String,
    pub modified_at: String,
    pub modified_text: String,
    pub kind_text: String,
    pub hidden: bool,
    pub readable: bool,
    pub symlink_target: String,
    pub mode_text: String,
    pub owner_text: String,
}

/// Map a `FilesystemEntry` from the unified filesystem service to a GUI row.
pub fn map_filesystem_row(entry: &FilesystemEntry) -> VolumeFileRow {
    let size_known = entry.size_bytes.is_some() && !entry.entry_type.is_directory();
    let size_bytes = entry.size_bytes.unwrap_or(0) as i64;
    VolumeFileRow {
        name: entry.display_name.clone(),
        display_name: entry.display_name.clone(),
        path: entry.path_display(),
        entry_type: entry.entry_type.as_str().into(),
        icon_name: icon_for_filesystem_entry(entry).into(),
        size_bytes,
        size_known,
        size_text: if entry.entry_type.is_directory() {
            "\u{2014}".into()
        } else if let Some(size) = entry.size_bytes {
            format::bytes(size)
        } else {
            "Unknown".into()
        },
        modified_at: entry
            .modified_at
            .map(|dt| dt.to_rfc3339())
            .unwrap_or_default(),
        modified_text: format_modified(entry.modified_at),
        kind_text: kind_text_filesystem(entry),
        hidden: entry.hidden,
        readable: entry.readable,
        symlink_target: entry.symlink_target_display.clone().unwrap_or_default(),
        mode_text: format_mode(entry.mode),
        owner_text: format_owner(entry.uid, entry.gid),
    }
}

fn icon_for_filesystem_entry(entry: &FilesystemEntry) -> &'static str {
    match entry.entry_type {
        FilesystemEntryType::Directory => "folder",
        FilesystemEntryType::SymbolicLink => "emblem-symbolic-link",
        FilesystemEntryType::Socket => "network-server",
        FilesystemEntryType::Fifo => "inode-fifo",
        FilesystemEntryType::BlockDevice | FilesystemEntryType::CharacterDevice => "drive-harddisk",
        FilesystemEntryType::Unknown => "unknown",
        FilesystemEntryType::RegularFile => icon_for_name(&entry.display_name),
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
        || lower.ends_with(".bmp")
        || lower.ends_with(".svg")
    {
        "image-x-generic"
    } else if lower.ends_with(".zip")
        || lower.ends_with(".tar")
        || lower.ends_with(".gz")
        || lower.ends_with(".tgz")
        || lower.ends_with(".xz")
        || lower.ends_with(".7z")
    {
        "package-x-generic"
    } else if lower.ends_with(".sh") || lower.ends_with(".bin") {
        "application-x-executable"
    } else if lower.ends_with(".txt")
        || lower.ends_with(".log")
        || lower.ends_with(".md")
        || lower.ends_with(".conf")
        || lower.ends_with(".ini")
        || lower.ends_with(".toml")
        || lower.ends_with(".yaml")
        || lower.ends_with(".yml")
        || lower.ends_with(".xml")
        || lower.ends_with(".csv")
        || lower.ends_with(".env")
    {
        "text-plain"
    } else {
        "text-x-generic"
    }
}

fn kind_text_filesystem(entry: &FilesystemEntry) -> String {
    match entry.entry_type {
        FilesystemEntryType::Directory => "Folder".into(),
        FilesystemEntryType::SymbolicLink => {
            if let Some(target) = &entry.symlink_target_display {
                format!("Symbolic Link \u{2192} {target}")
            } else {
                "Symbolic Link".into()
            }
        }
        FilesystemEntryType::Socket => "Socket".into(),
        FilesystemEntryType::Fifo => "FIFO".into(),
        FilesystemEntryType::BlockDevice => "Block Device".into(),
        FilesystemEntryType::CharacterDevice => "Character Device".into(),
        FilesystemEntryType::Unknown => "Unknown".into(),
        FilesystemEntryType::RegularFile => kind_for_file_name(&entry.display_name),
    }
}

fn kind_for_file_name(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".json") {
        "JSON Document".into()
    } else if lower.ends_with(".png") {
        "PNG Image".into()
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "JPEG Image".into()
    } else if lower.ends_with(".gif") {
        "GIF Image".into()
    } else if lower.ends_with(".webp") {
        "WebP Image".into()
    } else if lower.ends_with(".svg") {
        "SVG Image".into()
    } else if lower.ends_with(".txt") || lower.ends_with(".log") || lower.ends_with(".md") {
        "Text Document".into()
    } else {
        "File".into()
    }
}

fn format_modified(value: Option<DateTime<Utc>>) -> String {
    match value {
        Some(dt) => dt.format("%Y-%m-%d %H:%M").to_string(),
        None => "Unknown".into(),
    }
}

pub fn format_mode(mode: Option<u32>) -> String {
    match mode {
        Some(mode) => {
            let octal = format!("{mode:04o}");
            let symbolic = mode_to_symbolic(mode);
            format!("{octal} ({symbolic})")
        }
        None => "Unknown".into(),
    }
}

fn mode_to_symbolic(mode: u32) -> String {
    let mut out = String::with_capacity(9);
    for shift in [6, 3, 0] {
        let bits = (mode >> shift) & 0b111;
        out.push(if bits & 0b100 != 0 { 'r' } else { '-' });
        out.push(if bits & 0b010 != 0 { 'w' } else { '-' });
        out.push(if bits & 0b001 != 0 { 'x' } else { '-' });
    }
    out
}

fn format_owner(uid: Option<u32>, gid: Option<u32>) -> String {
    match (uid, gid) {
        (Some(uid), Some(gid)) => format!("{uid}:{gid}"),
        (Some(uid), None) => format!("{uid}:?"),
        (None, Some(gid)) => format!("?:{gid}"),
        (None, None) => "Unknown".into(),
    }
}
