use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;

use tuxstack_vfs::{
    DEFAULT_MAX_PATH_BYTES, FuseNameCodec, VfsError, VirtualFileName, VirtualPath, VirtualPathBytes,
};

#[test]
fn root_and_normalization_are_byte_oriented() {
    assert_eq!(
        VirtualPath::from_absolute(b"/").unwrap(),
        VirtualPath::root()
    );
    assert_eq!(
        VirtualPath::from_absolute(b"/etc/./docker/../hosts")
            .unwrap()
            .as_bytes(),
        b"/etc/hosts"
    );
    let non_utf8 = VirtualPath::from_absolute(b"/raw/\xff").unwrap();
    assert_eq!(non_utf8.components()[1].as_bytes(), b"\xff");
}

#[test]
fn names_reject_nul_dot_dotdot_slash_empty_and_overlong() {
    for invalid in [b"".as_slice(), b".", b"..", b"a/b", b"a\0b"] {
        assert!(
            VirtualFileName::new(invalid).is_err(),
            "accepted {invalid:?}"
        );
    }
    assert_eq!(
        VirtualFileName::new(vec![b'a'; 256]),
        Err(VfsError::NameTooLong)
    );
    assert!(VirtualFileName::new(vec![b'a'; 255]).is_ok());
    assert!(VirtualPathBytes::new(b"a\0b").is_err());
}

#[test]
fn paths_reject_relative_nul_root_escape_and_length_limit() {
    assert!(VirtualPath::from_absolute(b"relative").is_err());
    assert!(VirtualPath::from_absolute(b"/bad\0path").is_err());
    assert_eq!(
        VirtualPath::from_absolute(b"/../etc"),
        Err(VfsError::SymlinkEscape)
    );
    let mut overlong = vec![b'/'; DEFAULT_MAX_PATH_BYTES + 1];
    overlong[1..].fill(b'a');
    assert_eq!(
        VirtualPath::from_absolute(overlong),
        Err(VfsError::PathTooLong)
    );
}

#[test]
fn codec_matches_examples_preserves_unicode_and_roundtrips_raw_bytes() {
    let cases: &[(&[u8], &str)] = &[
        (b"postgres:17", "postgres%3A17"),
        (b"ghcr.io/org/app:latest", "ghcr.io%2Forg%2Fapp%3Alatest"),
        ("你好-世界".as_bytes(), "你好-世界"),
        (b".", "%2E"),
        (b"..", "%2E%2E"),
        (b"100%", "100%25"),
        (b"raw-\xff", "raw-%FF"),
        (b"control-\n", "control-%0A"),
    ];
    for (raw, expected) in cases {
        let encoded = FuseNameCodec::encode(raw).unwrap();
        assert_eq!(&encoded, expected);
        assert_eq!(FuseNameCodec::decode(&encoded).unwrap(), *raw);
        let name = VirtualFileName::new(encoded.as_bytes()).unwrap();
        assert_eq!(OsStr::from_bytes(name.as_bytes()), name.as_os_str());
    }
}

#[test]
fn malformed_or_nul_percent_encoding_is_rejected() {
    for malformed in ["%", "%0", "%GG", "%00"] {
        assert!(FuseNameCodec::decode(malformed).is_err());
    }
}

#[test]
fn collision_suffix_is_stable_distinct_and_separable() {
    let base = FuseNameCodec::encode(b"dev").unwrap();
    let first = FuseNameCodec::with_collision_suffix(&base, "container-one");
    assert_eq!(
        first,
        FuseNameCodec::with_collision_suffix(&base, "container-one")
    );
    assert_ne!(
        first,
        FuseNameCodec::with_collision_suffix(&base, "container-two")
    );
    assert_eq!(FuseNameCodec::split_collision_suffix(&first), base);
    assert_eq!(FuseNameCodec::decode(&base).unwrap(), b"dev");
}
