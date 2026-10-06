// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use super::{Ace, Buffer, Record, directory_names, kind, security};
use crate::capdir::Kind;

const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
const FILE_ATTRIBUTE_ARCHIVE: u32 = 0x20;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xa000_0003;
const IO_REPARSE_TAG_SYMLINK: u32 = 0xa000_000c;
const IO_REPARSE_TAG_CLOUD: u32 = 0x9000_001a;
const IO_REPARSE_TAG_APPEXECLINK: u32 = 0x8000_001b;

#[test]
fn an_app_execution_alias_is_its_own_kind_and_no_other_reparse_point_is_what_it_points_at() {
    let alias = FILE_ATTRIBUTE_ARCHIVE | FILE_ATTRIBUTE_REPARSE_POINT;
    for (attributes, tag, expected, what) in [
        (
            alias,
            IO_REPARSE_TAG_APPEXECLINK,
            Kind::ExecutionAlias,
            "an app execution alias, which only process creation follows and a file open fails on",
        ),
        (
            FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT,
            IO_REPARSE_TAG_APPEXECLINK,
            Kind::ExecutionAlias,
            "an app execution alias on a directory, which no path traverses either",
        ),
        (
            alias,
            IO_REPARSE_TAG_SYMLINK,
            Kind::Other,
            "a file symbolic link, never followed",
        ),
        (
            FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT,
            IO_REPARSE_TAG_SYMLINK,
            Kind::Other,
            "a directory symbolic link, never followed",
        ),
        (
            FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT,
            IO_REPARSE_TAG_MOUNT_POINT,
            Kind::Other,
            "a junction, never followed",
        ),
        (
            alias,
            IO_REPARSE_TAG_CLOUD,
            Kind::Other,
            "a cloud placeholder, whose content a filter supplies",
        ),
        (
            FILE_ATTRIBUTE_ARCHIVE,
            IO_REPARSE_TAG_APPEXECLINK,
            Kind::File,
            "a file without the reparse attribute, whose tag Windows leaves undefined",
        ),
        (
            FILE_ATTRIBUTE_DIRECTORY,
            IO_REPARSE_TAG_APPEXECLINK,
            Kind::Directory,
            "a directory without the reparse attribute, whose tag Windows leaves undefined",
        ),
        (FILE_ATTRIBUTE_ARCHIVE, 0, Kind::File, "a regular file"),
        (FILE_ATTRIBUTE_DIRECTORY, 0, Kind::Directory, "a directory"),
    ] {
        assert_eq!(
            kind(attributes, tag),
            expected,
            "{what}: attributes {attributes:#x}, reparse tag {tag:#x}"
        );
    }
}

fn put(bytes: &mut [u8], at: usize, value: &[u8]) {
    let end = at.checked_add(value.len()).expect("fixture fits");
    bytes
        .get_mut(at..end)
        .expect("fixture range")
        .copy_from_slice(value);
}

fn descriptor() -> Vec<u8> {
    let mut bytes = vec![0; 60];
    put(&mut bytes, 0, &[1, 0]);
    put(&mut bytes, 2, &0x9004_u16.to_le_bytes());
    put(&mut bytes, 4, &20_u32.to_le_bytes());
    put(&mut bytes, 16, &32_u32.to_le_bytes());
    let sid = [1, 1, 0, 0, 0, 0, 0, 5, 32, 0, 0, 0];
    put(&mut bytes, 20, &sid);
    put(&mut bytes, 32, &[2, 0]);
    put(&mut bytes, 34, &28_u16.to_le_bytes());
    put(&mut bytes, 36, &1_u16.to_le_bytes());
    put(&mut bytes, 42, &20_u16.to_le_bytes());
    put(&mut bytes, 44, &0x001f_01ff_u32.to_le_bytes());
    put(&mut bytes, 48, &sid);
    bytes
}

#[test]
fn a_security_descriptor_is_read_without_any_pointer_to_its_records() {
    let bytes = descriptor();
    let parsed = security(&bytes).expect("descriptor");
    assert!(parsed.protected);
    let owner = parsed.owner.sid_words().expect("owner");
    match parsed.grants.expect("DACL").as_slice() {
        [Ace::Allows { sid, mask }] => {
            assert_eq!(*mask, 0x001f_01ff);
            assert_eq!(sid.sid_words().expect("SID"), owner);
        }
        other => panic!("unexpected grants: {other:?}"),
    }
}

#[test]
fn every_truncated_descriptor_and_variable_record_is_refused() {
    let bytes = descriptor();
    for end in 0..bytes.len() {
        let result = security(bytes.get(..end).expect("prefix"));
        assert!(
            result.is_err(),
            "truncation at {end} must be refused: {result:?}"
        );
    }
    for (at, replacement) in [
        (4, u32::MAX.to_le_bytes().to_vec()),
        (16, u32::MAX.to_le_bytes().to_vec()),
        (34, 7_u16.to_le_bytes().to_vec()),
        (34, 8_u16.to_le_bytes().to_vec()),
        (36, 2_u16.to_le_bytes().to_vec()),
        (42, 0_u16.to_le_bytes().to_vec()),
        (42, 4_u16.to_le_bytes().to_vec()),
        (42, 16_u16.to_le_bytes().to_vec()),
        (42, u16::MAX.to_le_bytes().to_vec()),
        (48, vec![2]),
        (49, vec![16]),
    ] {
        let mut corrupted = bytes.clone();
        put(&mut corrupted, at, &replacement);
        assert!(security(&corrupted).is_err(), "corruption at {at}");
    }
}

#[test]
fn an_ace_cannot_borrow_its_sid_from_acl_slack() {
    let mut bytes = descriptor();
    put(&mut bytes, 42, &8_u16.to_le_bytes());
    assert!(
        security(&bytes).is_err(),
        "an ACE may not borrow its SID from ACL slack"
    );
}

#[test]
fn unknown_entries_and_absent_or_empty_dacls_keep_their_meaning() {
    let mut bytes = descriptor();
    put(&mut bytes, 40, &[1]);
    assert!(matches!(
        security(&bytes)
            .expect("unknown ACE")
            .grants
            .expect("DACL")
            .as_slice(),
        [Ace::Other]
    ));
    put(&mut bytes, 36, &0_u16.to_le_bytes());
    assert!(
        security(&bytes)
            .expect("empty DACL")
            .grants
            .expect("DACL")
            .is_empty()
    );
    put(&mut bytes, 16, &0_u32.to_le_bytes());
    assert!(security(&bytes).expect("null DACL").grants.is_none());
}

#[test]
fn a_token_pointer_is_only_an_address_into_the_owned_bytes() {
    let base = 4096_usize;
    let offset = size_of::<usize>();
    let sid = [1, 1, 0, 0, 0, 0, 0, 5, 32, 0, 0, 0];
    let mut bytes = base
        .checked_add(offset)
        .expect("address")
        .to_ne_bytes()
        .to_vec();
    bytes.extend_from_slice(&sid);
    assert_eq!(
        Record::new(&bytes)
            .token_sid(base)
            .expect("inside")
            .sid_words()
            .expect("SID")
            .len(),
        3
    );
    for address in [
        base.checked_sub(1).expect("before"),
        base,
        base.checked_add(bytes.len()).expect("end"),
        usize::MAX,
    ] {
        put(&mut bytes, 0, &address.to_ne_bytes());
        assert!(
            Record::new(&bytes).token_sid(base).is_err(),
            "address {address}"
        );
    }
    assert_eq!(
        Record::new(&[])
            .range(usize::MAX, 1)
            .expect_err("overflow")
            .kind(),
        std::io::ErrorKind::InvalidData
    );
}

#[test]
fn directory_names_cannot_cross_into_the_next_record() {
    let mut bytes = vec![0; 72];
    put(&mut bytes, 60, &2_u32.to_le_bytes());
    put(&mut bytes, 68, &97_u16.to_le_bytes());
    assert_eq!(directory_names(&bytes).expect("name"), [vec![97]]);
    let mut two = bytes.clone();
    two.extend_from_slice(&bytes);
    put(&mut two, 0, &72_u32.to_le_bytes());
    assert_eq!(
        directory_names(&two).expect("two names"),
        [vec![97], vec![97]]
    );
    for (at, value) in [
        (0, 4),
        (0, 68),
        (0, u32::MAX),
        (60, 1),
        (60, 8),
        (60, u32::MAX),
    ] {
        let mut corrupted = two.clone();
        put(&mut corrupted, at, &value.to_le_bytes());
        assert!(
            directory_names(&corrupted).is_err(),
            "offset {at}, value {value}"
        );
    }
}

#[test]
fn aligned_storage_never_reads_more_than_it_owns_or_the_os_wrote() {
    for length in 0..25 {
        let bytes: Vec<u8> = (0..length).collect();
        let mut buffer = Buffer::from_bytes(&bytes);
        assert_eq!(buffer.bytes(bytes.len()).expect("written bytes"), bytes);
        assert!(buffer.pointer().addr().is_multiple_of(8));
        assert_eq!(buffer.pointer(), buffer.output());
        let capacity = usize::try_from(buffer.capacity().expect("capacity")).expect("fits");
        assert_eq!(
            buffer
                .bytes(capacity.checked_add(1).expect("one past"))
                .expect_err("outside owner")
                .kind(),
            std::io::ErrorKind::InvalidData
        );
    }
    assert_eq!(Buffer::sized(9).bytes(9).expect("initialized"), vec![0; 9]);
    let mut bytes = [0; 8];
    super::put(&mut bytes, 4, &[1, 2, 3, 4]).expect("inside");
    assert_eq!(Record::new(&bytes).word(4).expect("word"), 0x0403_0201);
    assert_eq!(
        super::put(&mut bytes, 7, &[1, 2])
            .expect_err("short buffer")
            .kind(),
        std::io::ErrorKind::InvalidData
    );
    assert_eq!(
        super::put(&mut bytes, usize::MAX, &[1])
            .expect_err("overflow")
            .kind(),
        std::io::ErrorKind::InvalidData
    );
}
