//! `preview` command: bounded byte-range reads streamed as base64 chunks.
//!
//! Never reads a whole file: seeks to `--offset` and streams at most
//! `--limit-bytes` bytes in small chunks. Directories and special files
//! (FIFO/socket/device) are refused so a preview can never block on a
//! never-ending pipe or device.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

use tuxstack_fs_protocol::{HelperMessage, encode_base64};

use crate::error::{HelperError, Result};
use tuxstack_fs_protocol::HelperErrorCode;
use crate::path::{self, confine_to_root, resolve_token};
use crate::emit;

const CHUNK_SIZE: u64 = 32 * 1024;

pub fn run(args: &[String]) -> Result<()> {
    let flags = path::parse_flags(args)?;
    let raw_path = resolve_token(&flags.root, &flags.token)?;
    // Canonicalize and verify the resolved path stays inside the browse root
    // (blocks symlinks pointing outside the root).
    let safe = confine_to_root(&flags.root, &raw_path)?;

    let meta = std::fs::metadata(&safe).map_err(|error| HelperError::from_io(&safe, &error))?;
    if meta.is_dir() {
        return Err(HelperError::new(
            HelperErrorCode::IsDirectory,
            format!("is a directory: {}", safe.display()),
        ));
    }
    let ft = meta.file_type();
    if !ft.is_file() && !ft.is_symlink() {
        return Err(HelperError::new(
            HelperErrorCode::UnsupportedFileType,
            format!("unsupported file type: {}", safe.display()),
        ));
    }

    let size = meta.len();
    let offset = flags.offset;
    let limit = flags.read_limit;
    let end = offset.saturating_add(limit).min(size);
    let truncated = end < size;

    if limit == 0 || offset >= size {
        emit(&HelperMessage::PreviewChunk {
            data_b64: encode_base64(b""),
            offset,
            eof: true,
            truncated,
        });
        return Ok(());
    }

    let mut file = File::open(&safe).map_err(|error| HelperError::from_io(&safe, &error))?;
    if offset > 0 {
        file.seek(SeekFrom::Start(offset))
            .map_err(|error| HelperError::from_io(&safe, &error))?;
    }
    let mut remaining = end - offset;
    let mut position = offset;
    let mut buffer = vec![0u8; CHUNK_SIZE as usize];
    while remaining > 0 {
        let want = remaining.min(CHUNK_SIZE) as usize;
        let n = file
            .read(&mut buffer[..want])
            .map_err(|error| HelperError::from_io(&safe, &error))?;
        if n == 0 {
            break;
        }
        position += n as u64;
        remaining -= n as u64;
        emit(&HelperMessage::PreviewChunk {
            data_b64: encode_base64(&buffer[..n]),
            offset: position - n as u64,
            eof: remaining == 0,
            truncated,
        });
    }
    Ok(())
}
