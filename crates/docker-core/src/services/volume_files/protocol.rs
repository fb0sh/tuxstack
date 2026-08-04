//! Structured helper protocol for directory listing and stat.

use base64::{Engine as _, engine::general_purpose::STANDARD as B64};

/// Fixed listing script. Path, show-hidden flag, and max entries are argv
/// (`$1`, `$2`, `$3`) — never spliced into the script body.
///
/// Output lines:
/// `type|size|mtime|mode|uid|gid|name_b64|target_b64`
///
/// Ends with `TRUNCATED` when the entry cap is hit.
pub const LIST_SCRIPT: &str = r#"
set -eu
dir=$1
show_hidden=$2
max_entries=$3
if [ ! -e "$dir" ]; then
  echo MISSING >&2
  exit 2
fi
if [ ! -d "$dir" ]; then
  echo NOTDIR >&2
  exit 2
fi
if [ ! -r "$dir" ]; then
  echo UNREADABLE >&2
  exit 2
fi
count=0
# BusyBox find supports -print0.
find "$dir" -mindepth 1 -maxdepth 1 -print0 2>/dev/null | while IFS= read -r -d '' f; do
  base=${f##*/}
  if [ "$show_hidden" != "1" ]; then
    case "$base" in
      .*) continue ;;
    esac
  fi
  count=$((count + 1))
  if [ "$count" -gt "$max_entries" ]; then
    echo TRUNCATED
    break
  fi
  if [ -L "$f" ]; then
    t=l
    target=$(readlink -n "$f" 2>/dev/null || true)
  elif [ -d "$f" ]; then
    t=d
    target=
  elif [ -f "$f" ]; then
    t=f
    target=
  elif [ -S "$f" ]; then
    t=s
    target=
  elif [ -p "$f" ]; then
    t=p
    target=
  elif [ -b "$f" ]; then
    t=b
    target=
  elif [ -c "$f" ]; then
    t=c
    target=
  else
    t=u
    target=
  fi
  size=$(stat -c %s "$f" 2>/dev/null || echo)
  mtime=$(stat -c %Y "$f" 2>/dev/null || echo)
  mode=$(stat -c %a "$f" 2>/dev/null || echo)
  uid=$(stat -c %u "$f" 2>/dev/null || echo)
  gid=$(stat -c %g "$f" 2>/dev/null || echo)
  # BusyBox base64 may wrap lines; strip newlines.
  name_b64=$(printf '%s' "$base" | base64 | tr -d '\n')
  if [ -n "$target" ]; then
    target_b64=$(printf '%s' "$target" | base64 | tr -d '\n')
  else
    target_b64=
  fi
  printf '%s|%s|%s|%s|%s|%s|%s|%s\n' "$t" "$size" "$mtime" "$mode" "$uid" "$gid" "$name_b64" "$target_b64"
done
"#;

/// Stat a single path. Same line format as list, or MISSING / UNREADABLE.
pub const STAT_SCRIPT: &str = r#"
set -eu
f=$1
if [ ! -e "$f" ] && [ ! -L "$f" ]; then
  echo MISSING
  exit 0
fi
if [ -L "$f" ]; then
  t=l
  target=$(readlink -n "$f" 2>/dev/null || true)
elif [ -d "$f" ]; then
  t=d
  target=
elif [ -f "$f" ]; then
  t=f
  target=
elif [ -S "$f" ]; then
  t=s
  target=
elif [ -p "$f" ]; then
  t=p
  target=
elif [ -b "$f" ]; then
  t=b
  target=
elif [ -c "$f" ]; then
  t=c
  target=
else
  t=u
  target=
fi
if [ ! -r "$f" ] && [ ! -L "$f" ]; then
  echo UNREADABLE
  exit 0
fi
size=$(stat -c %s "$f" 2>/dev/null || echo)
mtime=$(stat -c %Y "$f" 2>/dev/null || echo)
mode=$(stat -c %a "$f" 2>/dev/null || echo)
uid=$(stat -c %u "$f" 2>/dev/null || echo)
gid=$(stat -c %g "$f" 2>/dev/null || echo)
base=${f##*/}
name_b64=$(printf '%s' "$base" | base64 | tr -d '\n')
if [ -n "${target:-}" ]; then
  target_b64=$(printf '%s' "$target" | base64 | tr -d '\n')
else
  target_b64=
fi
printf '%s|%s|%s|%s|%s|%s|%s|%s\n' "$t" "$size" "$mtime" "$mode" "$uid" "$gid" "$name_b64" "$target_b64"
"#;

/// Read at most N bytes from a path. Path is `$1` only (never string-spliced).
pub const PREVIEW_HEAD_SCRIPT: &str = r#"
set -eu
f=$1
limit=$2
if [ ! -e "$f" ] && [ ! -L "$f" ]; then
  echo MISSING >&2
  exit 2
fi
# BusyBox head supports -c.
head -c "$limit" "$f"
"#;

#[derive(Debug, Clone)]
pub struct ParsedListLine {
    pub type_code: String,
    pub size: Option<u64>,
    pub mtime: Option<i64>,
    pub mode: Option<u32>,
    pub uid: Option<u32>,
    pub gid: Option<u32>,
    pub name_b64: String,
    pub target_b64: String,
    pub readable: bool,
}

pub fn parse_list_line(line: &str) -> Result<ParsedListLine, String> {
    let mut parts = line.splitn(8, '|');
    let type_code = parts.next().ok_or("missing type")?.to_string();
    let size = parse_opt_u64(parts.next().unwrap_or(""));
    let mtime = parse_opt_i64(parts.next().unwrap_or(""));
    let mode = parse_opt_u32_octal(parts.next().unwrap_or(""));
    let uid = parse_opt_u32(parts.next().unwrap_or(""));
    let gid = parse_opt_u32(parts.next().unwrap_or(""));
    let name_b64 = parts.next().ok_or("missing name")?.to_string();
    let target_b64 = parts.next().unwrap_or("").to_string();
    if name_b64.is_empty() {
        return Err("empty name encoding".into());
    }
    Ok(ParsedListLine {
        type_code,
        size,
        mtime,
        mode,
        uid,
        gid,
        name_b64,
        target_b64,
        readable: true,
    })
}

pub fn decode_name(encoded: &str) -> Result<String, String> {
    let bytes = B64
        .decode(encoded.trim())
        .map_err(|error| format!("base64: {error}"))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn parse_opt_u64(value: &str) -> Option<u64> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        value.parse().ok()
    }
}

fn parse_opt_i64(value: &str) -> Option<i64> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        value.parse().ok()
    }
}

fn parse_opt_u32(value: &str) -> Option<u32> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        value.parse().ok()
    }
}

fn parse_opt_u32_octal(value: &str) -> Option<u32> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        u32::from_str_radix(value, 8)
            .ok()
            .or_else(|| value.parse().ok())
    }
}
