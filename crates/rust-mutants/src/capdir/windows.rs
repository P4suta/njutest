// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The capability directory on handle-relative NT opens, on a volume that deletes and renames the POSIX way (ADR 0037 decisions 3 to 7).

use std::fs::File;
use std::io;
use std::os::windows::fs::OpenOptionsExt as _;
use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle};
use std::path::Path;
use std::ptr;
use std::time::Duration;

use windows_sys::Wdk::Foundation::OBJECT_ATTRIBUTES;
use windows_sys::Wdk::Storage::FileSystem::{
    FILE_CREATE, FILE_DIRECTORY_FILE, FILE_NON_DIRECTORY_FILE, FILE_OPEN, FILE_OPEN_REPARSE_POINT,
    FILE_RENAME_IGNORE_READONLY_ATTRIBUTE, FILE_RENAME_INFORMATION, FILE_RENAME_POSIX_SEMANTICS,
    FILE_RENAME_REPLACE_IF_EXISTS, FILE_SYNCHRONOUS_IO_NONALERT, FileRenameInformationEx,
    NTCREATEFILE_CREATE_DISPOSITION, NTCREATEFILE_CREATE_OPTIONS, NtCreateFile,
    NtSetInformationFile,
};
use windows_sys::Win32::Foundation::{
    ERROR_NO_MORE_FILES, ERROR_SHARING_VIOLATION, HANDLE, NTSTATUS, OBJ_CASE_INSENSITIVE,
    RtlNtStatusToDosError, UNICODE_STRING,
};
use windows_sys::Win32::Security::{
    ACCESS_ALLOWED_ACE, ACE_FLAGS, ACL, ACL_REVISION, AddAccessAllowedAceEx, CONTAINER_INHERIT_ACE,
    CreateWellKnownSid, DACL_SECURITY_INFORMATION, GetKernelObjectSecurity, GetTokenInformation,
    InitializeAcl, InitializeSecurityDescriptor, OBJECT_INHERIT_ACE, OWNER_SECURITY_INFORMATION,
    PSECURITY_DESCRIPTOR, PSID, SE_DACL_PROTECTED, SECURITY_DESCRIPTOR, SECURITY_MAX_SID_SIZE,
    SetKernelObjectSecurity, SetSecurityDescriptorControl, SetSecurityDescriptorDacl,
    TOKEN_INFORMATION_CLASS, TOKEN_QUERY, TokenOwner, TokenUser, WELL_KNOWN_SID_TYPE,
    WinBuiltinAdministratorsSid, WinLocalSystemSid,
};
use windows_sys::Win32::Storage::FileSystem::{
    DELETE, FILE_ADD_FILE, FILE_ALL_ACCESS, FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_TAG_INFO,
    FILE_BASIC_INFO, FILE_DISPOSITION_FLAG_DELETE, FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE,
    FILE_DISPOSITION_FLAG_POSIX_SEMANTICS, FILE_DISPOSITION_INFO_EX, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_FLAGS_AND_ATTRIBUTES, FILE_GENERIC_READ, FILE_GENERIC_WRITE,
    FILE_ID_128, FILE_ID_INFO, FILE_INFO_BY_HANDLE_CLASS, FILE_LIST_DIRECTORY,
    FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_MODE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    FILE_STANDARD_INFO, FILE_TRAVERSE, FILE_WRITE_DATA, FileAttributeTagInfo, FileBasicInfo,
    FileDispositionInfoEx, FileFullDirectoryInfo, FileFullDirectoryRestartInfo, FileIdInfo,
    FileStandardInfo, FlushFileBuffers, GetFileInformationByHandleEx,
    GetVolumeInformationByHandleW, READ_CONTROL, SYNCHRONIZE, SetFileInformationByHandle,
    WRITE_DAC,
};
use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;
use windows_sys::Win32::System::SystemServices::{
    FILE_SUPPORTS_POSIX_UNLINK_RENAME, SECURITY_DESCRIPTOR_REVISION,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use super::records::{self, Ace, Buffer, Record};
use super::{Identity, Kind, Name, Privacy, REMOVAL_DEPTH, Status};

/// What a held directory may do: list itself, be passed through, and show its attributes and its security.
const DIRECTORY_ACCESS: u32 =
    FILE_LIST_DIRECTORY | FILE_TRAVERSE | FILE_READ_ATTRIBUTES | READ_CONTROL | SYNCHRONIZE;

/// What a removal or a rename holds the entry with, which is nothing beyond taking its name away.
const MOVE_ACCESS: u32 = DELETE | FILE_READ_ATTRIBUTES | SYNCHRONIZE;

/// Every handle here lets another open, rename or remove the same entry, as a Unix descriptor never stops anyone.
const SHARE_ALL: FILE_SHARE_MODE = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;

/// How an object named by a path is opened: a directory as well as a file, and as the object itself rather than what a link points at.
const BY_PATH: FILE_FLAGS_AND_ATTRIBUTES =
    FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT;

/// How long to wait before each further try of a rename or a removal another process is holding the entry against (decision 7).
const WAITS: [Duration; 9] = [
    Duration::from_millis(1),
    Duration::from_millis(2),
    Duration::from_millis(4),
    Duration::from_millis(8),
    Duration::from_millis(16),
    Duration::from_millis(32),
    Duration::from_millis(64),
    Duration::from_millis(128),
    Duration::from_millis(256),
];

/// The file systems a report is kept on, each only where it says it deletes and renames the POSIX way (decision 4).
const DURABLE_SYSTEMS: [&str; 2] = ["NTFS", "ReFS"];

pub(super) fn open(path: &Path) -> io::Result<File> {
    let directory = std::fs::OpenOptions::new()
        .access_mode(DIRECTORY_ACCESS)
        .share_mode(SHARE_ALL)
        .custom_flags(BY_PATH)
        .open(path)?;
    match kind_of(&directory)? {
        Kind::Directory => {}
        Kind::ExecutionAlias | Kind::Other => return Err(link_refused()),
        Kind::File => {
            return Err(io::Error::new(
                io::ErrorKind::NotADirectory,
                "a capability directory is opened on a directory",
            ));
        }
    }
    volume_verdict(&volume_of(&directory)?)?;
    Ok(directory)
}

pub(super) fn open_dir(dir: &File, name: Name<'_>) -> io::Result<File> {
    let opened = create(
        dir,
        &wide(name),
        &Create {
            access: DIRECTORY_ACCESS,
            disposition: FILE_OPEN,
            options: FILE_DIRECTORY_FILE | FILE_OPEN_REPARSE_POINT,
            security: None,
        },
    )?;
    match kind_of(&opened)? {
        Kind::Directory => Ok(opened),
        Kind::ExecutionAlias | Kind::Other | Kind::File => Err(link_refused()),
    }
}

pub(super) fn open_file(dir: &File, name: Name<'_>) -> io::Result<File> {
    let (opened, kind) = open_entry(dir, name)?;
    match kind {
        Kind::File | Kind::Directory => Ok(opened),
        Kind::ExecutionAlias | Kind::Other => Err(link_refused()),
    }
}

pub(super) fn open_entry(dir: &File, name: Name<'_>) -> io::Result<(File, Kind)> {
    let opened = create(
        dir,
        &wide(name),
        &Create {
            access: FILE_GENERIC_READ,
            disposition: FILE_OPEN,
            options: FILE_OPEN_REPARSE_POINT,
            security: None,
        },
    )?;
    let kind = kind_of(&opened)?;
    Ok((opened, kind))
}

pub(super) fn open_file_at(path: &Path) -> io::Result<File> {
    let opened = std::fs::OpenOptions::new()
        .access_mode(FILE_GENERIC_READ)
        .share_mode(SHARE_ALL)
        .custom_flags(BY_PATH)
        .open(path)?;
    match kind_of(&opened)? {
        Kind::File | Kind::Directory => Ok(opened),
        Kind::ExecutionAlias | Kind::Other => Err(link_refused()),
    }
}

pub(super) fn create_file(dir: &File, name: Name<'_>) -> io::Result<File> {
    let private = Private::made(Inherited::Not)?;
    create(
        dir,
        &wide(name),
        &Create {
            access: FILE_GENERIC_WRITE | FILE_READ_ATTRIBUTES,
            disposition: FILE_CREATE,
            options: FILE_NON_DIRECTORY_FILE,
            security: Some(&private),
        },
    )
}

pub(super) fn create_private_dir(dir: &File, name: Name<'_>) -> io::Result<File> {
    let private = Private::made(Inherited::ByWhatIsMadeInside)?;
    create(
        dir,
        &wide(name),
        &Create {
            access: DIRECTORY_ACCESS,
            disposition: FILE_CREATE,
            options: FILE_DIRECTORY_FILE,
            security: Some(&private),
        },
    )
}

pub(super) fn file_status(file: &File) -> io::Result<Status> {
    let mut id = FILE_ID_INFO {
        VolumeSerialNumber: 0,
        FileId: FILE_ID_128 {
            Identifier: [0; 16],
        },
    };
    by_handle(file, FileIdInfo, &mut id)?;
    let mut standard = FILE_STANDARD_INFO {
        AllocationSize: 0,
        EndOfFile: 0,
        NumberOfLinks: 0,
        DeletePending: false,
        Directory: false,
    };
    by_handle(file, FileStandardInfo, &mut standard)?;
    let len = u64::try_from(standard.EndOfFile)
        .map_err(|_negative| io::Error::other("a file size is negative"))?;
    Ok(Status {
        identity: Identity {
            volume: id.VolumeSerialNumber,
            object: u128::from_le_bytes(id.FileId.Identifier),
        },
        kind: kind_of(file)?,
        len,
    })
}

pub(super) fn change_time(file: &File) -> io::Result<i64> {
    let mut basic = FILE_BASIC_INFO {
        CreationTime: 0,
        LastAccessTime: 0,
        LastWriteTime: 0,
        ChangeTime: 0,
        FileAttributes: 0,
    };
    by_handle(file, FileBasicInfo, &mut basic)?;
    Ok(basic.ChangeTime)
}

pub(super) fn status_at(dir: &File, name: Name<'_>) -> io::Result<Option<Status>> {
    let opened = create(
        dir,
        &wide(name),
        &Create {
            access: FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            disposition: FILE_OPEN,
            options: FILE_OPEN_REPARSE_POINT,
            security: None,
        },
    );
    match opened {
        Ok(opened) => file_status(&opened).map(Some),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

pub(super) fn rename_noreplace(
    from_dir: &File,
    from: Name<'_>,
    to_dir: &File,
    to: Name<'_>,
) -> io::Result<()> {
    rename((from_dir, from), (to_dir, to), 0)
}

pub(super) fn rename_replace(
    from_dir: &File,
    from: Name<'_>,
    to_dir: &File,
    to: Name<'_>,
) -> io::Result<()> {
    rename(
        (from_dir, from),
        (to_dir, to),
        FILE_RENAME_REPLACE_IF_EXISTS
            | FILE_RENAME_POSIX_SEMANTICS
            | FILE_RENAME_IGNORE_READONLY_ATTRIBUTE,
    )
}

pub(super) fn remove(dir: &File, name: Name<'_>, directory: bool) -> io::Result<()> {
    let spelled = wide(name);
    retried(|| {
        let entry = create(dir, &spelled, &Create::to_move())?;
        match (kind_of(&entry)?, directory) {
            (Kind::Directory, true) | (Kind::File | Kind::ExecutionAlias | Kind::Other, false) => {
                dispose(&entry)
            }
            (Kind::Directory, false) => Err(io::Error::new(
                io::ErrorKind::IsADirectory,
                "a directory is removed as a directory",
            )),
            (Kind::File | Kind::ExecutionAlias | Kind::Other, true) => Err(io::Error::new(
                io::ErrorKind::NotADirectory,
                "only a directory is removed as one",
            )),
        }
    })
}

pub(super) fn sync(dir: &File) -> io::Result<()> {
    flush(&reopen(dir, FILE_ADD_FILE, FILE_DIRECTORY_FILE)?)
}

pub(super) fn sync_file(file: &File) -> io::Result<()> {
    flush(&reopen(file, FILE_WRITE_DATA, FILE_NON_DIRECTORY_FILE)?)
}

pub(super) fn entries(dir: &File) -> io::Result<Vec<String>> {
    listed(dir)?
        .into_iter()
        .map(|name| {
            String::from_utf16(&name).map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("a directory entry is not UTF-16: {error}"),
                )
            })
        })
        .collect()
}

pub(super) fn privacy(dir: &File) -> io::Result<Privacy> {
    let me = Me::now()?;
    let security = Security::of(dir)?;
    Ok(privacy_of(&security, &me, &trusted(&me)?))
}

pub(super) fn restrict_to_owner(dir: &File) -> io::Result<()> {
    let private = Private::made(Inherited::ByWhatIsMadeInside)?;
    let writer = reopen(dir, WRITE_DAC | READ_CONTROL, FILE_DIRECTORY_FILE)?;
    #[expect(unsafe_code, reason = "SetKernelObjectSecurity has no safe binding")]
    let set = unsafe {
        SetKernelObjectSecurity(
            writer.as_raw_handle(),
            DACL_SECURITY_INFORMATION,
            private.descriptor(),
        )
    };
    succeeded(set)
}

pub(super) fn remove_contents(dir: &File) -> io::Result<()> {
    remove_contents_at(dir, 0)
}

/// How one entry is opened or made relative to the directory that holds it.
struct Create<'a> {
    access: u32,
    disposition: NTCREATEFILE_CREATE_DISPOSITION,
    options: NTCREATEFILE_CREATE_OPTIONS,
    security: Option<&'a Private>,
}

impl Create<'_> {
    /// The entry itself, link or not, held only to take its name away.
    const fn to_move() -> Self {
        Self {
            access: MOVE_ACCESS,
            disposition: FILE_OPEN,
            options: FILE_OPEN_REPARSE_POINT,
            security: None,
        }
    }
}

/// Opens or makes `name` relative to the directory `parent` holds, which is `openat` on Windows: the parent is the object held, and nothing a name was pointed at since.
fn create(parent: &File, name: &[u16], how: &Create<'_>) -> io::Result<File> {
    let length = u16::try_from(size_of_val(name)).map_err(|_outside| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "a name is too long to be one component",
        )
    })?;
    let spelling: Vec<u8> = name.iter().flat_map(|unit| unit.to_le_bytes()).collect();
    let name_buffer = Buffer::from_bytes(&spelling);
    let object_name = UNICODE_STRING {
        Length: length,
        MaximumLength: length,
        Buffer: name_buffer.pointer().cast(),
    };
    let object = OBJECT_ATTRIBUTES {
        Length: size_as_u32::<OBJECT_ATTRIBUTES>()?,
        RootDirectory: parent.as_raw_handle(),
        ObjectName: &raw const object_name,
        Attributes: OBJ_CASE_INSENSITIVE,
        SecurityDescriptor: match how.security {
            Some(private) => private.descriptor().cast_const().cast(),
            None => ptr::null(),
        },
        SecurityQualityOfService: ptr::null(),
    };
    let mut handle: HANDLE = ptr::null_mut();
    let mut status_block = IO_STATUS_BLOCK::default();
    #[expect(unsafe_code, reason = "NtCreateFile has no safe binding")]
    let status = unsafe {
        NtCreateFile(
            &raw mut handle,
            how.access | SYNCHRONIZE,
            &raw const object,
            &raw mut status_block,
            ptr::null(),
            FILE_ATTRIBUTE_NORMAL,
            SHARE_ALL,
            how.disposition,
            how.options | FILE_SYNCHRONOUS_IO_NONALERT,
            ptr::null(),
            0,
        )
    };
    succeeded_nt(status)?;
    #[expect(
        unsafe_code,
        reason = "NtCreateFile succeeded, so the handle is open and owned by nothing else"
    )]
    let owned = unsafe { File::from_raw_handle(handle) };
    Ok(owned)
}

/// A second handle on the object `held` is, asking for other access: an open of no name relative to it, the object itself and no name looked up again, which `ReOpenFile` refuses for a directory.
fn reopen(held: &File, access: u32, options: NTCREATEFILE_CREATE_OPTIONS) -> io::Result<File> {
    create(
        held,
        &[],
        &Create {
            access,
            disposition: FILE_OPEN,
            options: options | FILE_OPEN_REPARSE_POINT,
            security: None,
        },
    )
}

/// Fills `info` with what the handle says about itself in the class `class` names, which must be the class `T` is laid out as.
fn by_handle<T>(file: &File, class: FILE_INFO_BY_HANDLE_CLASS, info: &mut T) -> io::Result<()> {
    let size = size_as_u32::<T>()?;
    #[expect(
        unsafe_code,
        reason = "GetFileInformationByHandleEx has no safe binding"
    )]
    let read = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            class,
            ptr::from_mut(info).cast(),
            size,
        )
    };
    succeeded(read)
}

/// What the open handle is, read without following it: a reparse point of any tag is never what it points at.
fn kind_of(file: &File) -> io::Result<Kind> {
    let mut tag = FILE_ATTRIBUTE_TAG_INFO {
        FileAttributes: 0,
        ReparseTag: 0,
    };
    by_handle(file, FileAttributeTagInfo, &mut tag)?;
    Ok(records::kind(tag.FileAttributes, tag.ReparseTag))
}

fn link_refused() -> io::Error {
    io::Error::other("a link or another reparse point is never followed")
}

/// Which file system the handle's volume is, and whether it deletes and renames the POSIX way.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Volume {
    system: String,
    posix: bool,
}

fn volume_of(directory: &File) -> io::Result<Volume> {
    let mut flags: u32 = 0;
    let mut system = [0_u16; 64];
    let capacity = u32::try_from(system.len())
        .map_err(|_outside| io::Error::other("a file system name buffer is too long"))?;
    #[expect(
        unsafe_code,
        reason = "GetVolumeInformationByHandleW has no safe binding"
    )]
    let asked = unsafe {
        GetVolumeInformationByHandleW(
            directory.as_raw_handle(),
            ptr::null_mut(),
            0,
            ptr::null_mut(),
            ptr::null_mut(),
            &raw mut flags,
            system.as_mut_ptr(),
            capacity,
        )
    };
    succeeded(asked)?;
    let named = match system.iter().position(|unit| *unit == 0) {
        Some(terminator) => system.split_at(terminator).0,
        None => system.as_slice(),
    };
    let system = String::from_utf16(named).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("a file system name is not UTF-16: {error}"),
        )
    })?;
    Ok(Volume {
        system,
        posix: flags & FILE_SUPPORTS_POSIX_UNLINK_RENAME != 0,
    })
}

/// Refuses a volume anything is kept on unless it is NTFS or `ReFS` and deletes and renames the POSIX way, naming what it is.
fn volume_verdict(volume: &Volume) -> io::Result<()> {
    let named = DURABLE_SYSTEMS.contains(&volume.system.as_str());
    if named && volume.posix {
        return Ok(());
    }
    let lacks = if named {
        "which does not say it deletes and renames the POSIX way"
    } else {
        "which is not NTFS or ReFS"
    };
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        format!(
            "the volume is {}, {lacks}; a report is kept only on NTFS or ReFS with POSIX unlink and rename semantics",
            volume.system
        ),
    ))
}

/// Moves `from`, named in the directory it pairs with, to `to` in the directory that one pairs with, under `flags`.
fn rename(from: (&File, Name<'_>), to: (&File, Name<'_>), flags: u32) -> io::Result<()> {
    let (from_dir, from) = from;
    let (to_dir, to) = to;
    let source = wide(from);
    let target = wide(to);
    retried(|| {
        let held = create(from_dir, &source, &Create::to_move())?;
        rename_held(&held, to_dir, &target, flags)
    })
}

/// Renames the object `held` is to `target` in `to_dir`, which is `renameat` whose source is the object rather than a name.
fn rename_held(held: &File, to_dir: &File, target: &[u16], flags: u32) -> io::Result<()> {
    let name_at = std::mem::offset_of!(FILE_RENAME_INFORMATION, FileName);
    let name_bytes = size_of_val(target);
    let length = size_of::<FILE_RENAME_INFORMATION>()
        .checked_add(name_bytes)
        .ok_or_else(|| io::Error::other("a rename cannot be described"))?;
    let mut bytes = vec![0; length];
    records::put(&mut bytes, 0, &flags.to_le_bytes())?;
    records::put(
        &mut bytes,
        std::mem::offset_of!(FILE_RENAME_INFORMATION, RootDirectory),
        &to_dir.as_raw_handle().addr().to_ne_bytes(),
    )?;
    records::put(
        &mut bytes,
        std::mem::offset_of!(FILE_RENAME_INFORMATION, FileNameLength),
        &u32::try_from(name_bytes)
            .map_err(|_outside| io::Error::other("a name is too long to rename to"))?
            .to_le_bytes(),
    )?;
    let spelled: Vec<u8> = target.iter().flat_map(|unit| unit.to_le_bytes()).collect();
    records::put(&mut bytes, name_at, &spelled)?;
    let buffer = Buffer::from_bytes(&bytes);
    let described = u32::try_from(length)
        .map_err(|_outside| io::Error::other("a rename cannot be described"))?;
    let mut status_block = IO_STATUS_BLOCK::default();
    #[expect(unsafe_code, reason = "NtSetInformationFile has no safe binding")]
    let status = unsafe {
        NtSetInformationFile(
            held.as_raw_handle(),
            &raw mut status_block,
            buffer.pointer(),
            described,
            FileRenameInformationEx,
        )
    };
    succeeded_nt(status)
}

/// Removes the entry `held` is, with POSIX semantics: its name is gone now, whoever else still holds it open.
fn dispose(held: &File) -> io::Result<()> {
    let disposition = FILE_DISPOSITION_INFO_EX {
        Flags: FILE_DISPOSITION_FLAG_DELETE
            | FILE_DISPOSITION_FLAG_POSIX_SEMANTICS
            | FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE,
    };
    let size = size_as_u32::<FILE_DISPOSITION_INFO_EX>()?;
    #[expect(unsafe_code, reason = "SetFileInformationByHandle has no safe binding")]
    let set = unsafe {
        SetFileInformationByHandle(
            held.as_raw_handle(),
            FileDispositionInfoEx,
            ptr::from_ref(&disposition).cast(),
            size,
        )
    };
    succeeded(set)
}

fn flush(writer: &File) -> io::Result<()> {
    #[expect(unsafe_code, reason = "FlushFileBuffers has no safe binding")]
    let flushed = unsafe { FlushFileBuffers(writer.as_raw_handle()) };
    succeeded(flushed)
}

/// Tries `operation` again while another process holds the entry without sharing it, and names that as the cause when it still does (decision 7).
fn retried<T, F>(mut operation: F) -> io::Result<T>
where
    F: FnMut() -> io::Result<T>,
{
    for wait in WAITS {
        match operation() {
            Err(error) if sharing_violation(&error) => std::thread::sleep(wait),
            settled => return settled,
        }
    }
    match operation() {
        Err(error) if sharing_violation(&error) => Err(io::Error::new(
            error.kind(),
            format!(
                "another process held the entry without sharing it through {} tries: {error}",
                WAITS.len()
            ),
        )),
        settled => settled,
    }
}

fn sharing_violation(error: &io::Error) -> bool {
    match (error.raw_os_error(), i32::try_from(ERROR_SHARING_VIOLATION)) {
        (Some(raw), Ok(sharing)) => raw == sharing,
        (Some(_) | None, Ok(_) | Err(_)) => false,
    }
}

/// The names `dir` holds as the system spells them, without `.` and `..`, read through a handle of their own so no other listing moves this one.
fn listed(dir: &File) -> io::Result<Vec<Vec<u16>>> {
    const BATCH_BYTES: usize = 65536;
    let scan = reopen(dir, FILE_LIST_DIRECTORY, FILE_DIRECTORY_FILE)?;
    let mut buffer = Buffer::sized(BATCH_BYTES);
    let capacity = buffer.capacity()?;
    let mut class = FileFullDirectoryRestartInfo;
    let mut names = Vec::new();
    loop {
        #[expect(
            unsafe_code,
            reason = "GetFileInformationByHandleEx has no safe binding"
        )]
        let read = unsafe {
            GetFileInformationByHandleEx(scan.as_raw_handle(), class, buffer.output(), capacity)
        };
        if read == 0 {
            let error = io::Error::last_os_error();
            return match (error.raw_os_error(), i32::try_from(ERROR_NO_MORE_FILES)) {
                (Some(raw), Ok(ended)) if raw == ended => Ok(names),
                (Some(_) | None, Ok(_) | Err(_)) => Err(error),
            };
        }
        class = FileFullDirectoryInfo;
        let bytes = buffer.bytes(BATCH_BYTES)?;
        names.extend(
            records::directory_names(&bytes)?
                .into_iter()
                .filter(|name| !matches!(name.as_slice(), [46] | [46, 46])),
        );
    }
}

fn remove_contents_at(dir: &File, depth: usize) -> io::Result<()> {
    for name in listed(dir)? {
        remove_entry(dir, &name, depth)?;
    }
    sync(dir)
}

/// Removes the entry `name` as the object it was opened as, which is the object removed: a link is removed as a link, and one gone before it was reached counts as removed.
fn remove_entry(dir: &File, name: &[u16], depth: usize) -> io::Result<()> {
    retried(|| {
        let entry = match create(dir, name, &Create::to_move()) {
            Ok(entry) => entry,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        if kind_of(&entry)? == Kind::Directory {
            if depth >= REMOVAL_DEPTH {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("a tree deeper than {REMOVAL_DEPTH} directories is not emptied"),
                ));
            }
            let deeper = depth
                .checked_add(1)
                .ok_or_else(|| io::Error::other("the removal depth cannot be counted"))?;
            let inside = reopen(
                &entry,
                FILE_LIST_DIRECTORY | FILE_TRAVERSE | FILE_READ_ATTRIBUTES,
                FILE_DIRECTORY_FILE,
            )?;
            remove_contents_at(&inside, deeper)?;
        }
        dispose(&entry)
    })
}

/// A security identifier copied out of whatever held it, in words, which is how the system lays one out.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Sid {
    held: Buffer,
    length: usize,
}

impl Sid {
    fn copy(record: Record<'_>) -> io::Result<Self> {
        let bytes: Vec<u8> = record
            .sid_words()?
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect();
        Ok(Self {
            length: bytes.len(),
            held: Buffer::from_bytes(&bytes),
        })
    }

    /// The identifier the system gives the principal `kind` names.
    fn well_known(kind: WELL_KNOWN_SID_TYPE) -> io::Result<Self> {
        let mut size = SECURITY_MAX_SID_SIZE;
        let mut held = Buffer::sized(
            usize::try_from(size)
                .map_err(|_outside| io::Error::other("a SID is too long to hold"))?,
        );
        #[expect(unsafe_code, reason = "CreateWellKnownSid has no safe binding")]
        let made =
            unsafe { CreateWellKnownSid(kind, ptr::null_mut(), held.output(), &raw mut size) };
        succeeded(made)?;
        let bytes = held.bytes(
            usize::try_from(size)
                .map_err(|_outside| io::Error::other("a SID is too long to hold"))?,
        )?;
        Self::copy(Record::new(&bytes))
    }

    const fn as_psid(&self) -> PSID {
        self.held.pointer()
    }
}

/// Who this process makes objects as: its user, and the owner the system gives what it makes, which is the administrators' group for an elevated administrator.
struct Me {
    user: Sid,
    owner: Sid,
}

impl Me {
    fn now() -> io::Result<Self> {
        let mut token: HANDLE = ptr::null_mut();
        #[expect(unsafe_code, reason = "OpenProcessToken has no safe binding")]
        let opened = unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut token) };
        succeeded(opened)?;
        #[expect(
            unsafe_code,
            reason = "OpenProcessToken succeeded, so the handle is open and owned by nothing else"
        )]
        let token = unsafe { OwnedHandle::from_raw_handle(token) };
        let user = token_sid(&token, TokenUser)?;
        let owner = token_sid(&token, TokenOwner)?;
        Ok(Self { user, owner })
    }
}

/// The identifier the token's `class` names, copied only from the bytes the system wrote into its owned buffer.
fn token_sid(token: &OwnedHandle, class: TOKEN_INFORMATION_CLASS) -> io::Result<Sid> {
    let mut needed: u32 = 0;
    #[expect(unsafe_code, reason = "GetTokenInformation has no safe binding")]
    let sized = unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            class,
            ptr::null_mut(),
            0,
            &raw mut needed,
        )
    };
    if sized != 0 || needed == 0 {
        return Err(io::Error::other(
            "the process token did not say how much it holds",
        ));
    }
    let mut buffer = Buffer::sized(
        usize::try_from(needed)
            .map_err(|_outside| io::Error::other("the process token holds too much"))?,
    );
    let capacity = buffer.capacity()?;
    #[expect(unsafe_code, reason = "GetTokenInformation has no safe binding")]
    let read = unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            class,
            buffer.output(),
            capacity,
            &raw mut needed,
        )
    };
    succeeded(read)?;
    let bytes = buffer.bytes(
        usize::try_from(needed)
            .map_err(|_outside| io::Error::other("the process token holds too much"))?,
    )?;
    Sid::copy(Record::new(&bytes).token_sid(buffer.pointer().addr())?)
}

/// The principals a private directory admits: this process's user, the system, and the administrators (decision 5).
fn trusted(me: &Me) -> io::Result<[Sid; 3]> {
    Ok([
        me.user.clone(),
        Sid::well_known(WinLocalSystemSid)?,
        Sid::well_known(WinBuiltinAdministratorsSid)?,
    ])
}

/// One entry of an access control list, as far as privacy asks: whom it allows, and how much, or that it is some other kind.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Grant {
    Allows { sid: Sid, mask: u32 },
    Other,
}

/// Who owns a directory and whom its access control list admits.
#[derive(Debug)]
struct Security {
    owner: Sid,
    protected: bool,
    grants: Option<Vec<Grant>>,
}

impl Security {
    fn of(dir: &File) -> io::Result<Self> {
        let requested = OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION;
        let mut needed = 0;
        #[expect(unsafe_code, reason = "GetKernelObjectSecurity has no safe binding")]
        let sized = unsafe {
            GetKernelObjectSecurity(
                dir.as_raw_handle(),
                requested,
                ptr::null_mut(),
                0,
                &raw mut needed,
            )
        };
        if sized != 0 || needed == 0 {
            return Err(io::Error::other(
                "the security descriptor did not say how much it holds",
            ));
        }
        let mut buffer = Buffer::sized(
            usize::try_from(needed)
                .map_err(|_outside| io::Error::other("the security descriptor holds too much"))?,
        );
        let capacity = buffer.capacity()?;
        #[expect(unsafe_code, reason = "GetKernelObjectSecurity has no safe binding")]
        let read = unsafe {
            GetKernelObjectSecurity(
                dir.as_raw_handle(),
                requested,
                buffer.output(),
                capacity,
                &raw mut needed,
            )
        };
        succeeded(read)?;
        let bytes = buffer
            .bytes(usize::try_from(needed).map_err(|_outside| {
                io::Error::other("the security descriptor holds too much")
            })?)?;
        let parsed = records::security(&bytes)?;
        let grants = parsed
            .grants
            .map(|grants| {
                grants
                    .into_iter()
                    .map(|grant| match grant {
                        Ace::Allows { sid, mask } => Ok(Grant::Allows {
                            sid: Sid::copy(sid)?,
                            mask,
                        }),
                        Ace::Other => Ok(Grant::Other),
                    })
                    .collect::<io::Result<Vec<Grant>>>()
            })
            .transpose()?;
        Ok(Self {
            owner: Sid::copy(parsed.owner)?,
            protected: parsed.protected,
            grants,
        })
    }
}

/// Who may reach into a directory with `security`, asked by `me`: owner-only is a protected list, which nothing is inherited into later, whose every entry allows one of `trusted` and which gives this user everything.
fn privacy_of(security: &Security, me: &Me, trusted: &[Sid; 3]) -> Privacy {
    if ![&me.user, &me.owner].contains(&&security.owner) {
        return Privacy::ForeignOwner;
    }
    let Some(grants) = &security.grants else {
        return Privacy::Loose;
    };
    let admitted = grants.iter().all(|grant| match grant {
        Grant::Allows { sid, .. } => trusted.contains(sid),
        Grant::Other => false,
    });
    let owned = grants.iter().any(|grant| match grant {
        Grant::Allows { sid, mask } => *sid == me.user && mask & FILE_ALL_ACCESS == FILE_ALL_ACCESS,
        Grant::Other => false,
    });
    if security.protected && admitted && owned {
        Privacy::OwnerOnly
    } else {
        Privacy::Loose
    }
}

/// Whether what a private entry grants is also granted to what is made inside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Inherited {
    Not,
    ByWhatIsMadeInside,
}

/// A security descriptor whose protected access control list admits this user, the system and the administrators alone (decision 5).
struct Private {
    acl: Buffer,
    descriptor: SECURITY_DESCRIPTOR,
}

impl Private {
    fn made(inherited: Inherited) -> io::Result<Self> {
        let me = Me::now()?;
        let trusted = trusted(&me)?;
        let flags: ACE_FLAGS = match inherited {
            Inherited::Not => 0,
            Inherited::ByWhatIsMadeInside => OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE,
        };
        let entry = size_of::<ACCESS_ALLOWED_ACE>()
            .checked_sub(size_of::<u32>())
            .ok_or_else(|| io::Error::other("an access control entry cannot be sized"))?;
        let mut length = size_of::<ACL>();
        for sid in &trusted {
            length = length
                .checked_add(entry)
                .and_then(|with| with.checked_add(sid.length))
                .ok_or_else(|| io::Error::other("an access control list cannot be sized"))?;
        }
        let mut acl = Buffer::sized(length);
        let capacity = acl.capacity()?;
        #[expect(unsafe_code, reason = "InitializeAcl has no safe binding")]
        let initialized = unsafe { InitializeAcl(acl.output().cast(), capacity, ACL_REVISION) };
        succeeded(initialized)?;
        for sid in &trusted {
            #[expect(unsafe_code, reason = "AddAccessAllowedAceEx has no safe binding")]
            let added = unsafe {
                AddAccessAllowedAceEx(
                    acl.output().cast(),
                    ACL_REVISION,
                    flags,
                    FILE_ALL_ACCESS,
                    sid.as_psid(),
                )
            };
            succeeded(added)?;
        }
        let mut private = Self {
            acl,
            descriptor: SECURITY_DESCRIPTOR {
                Revision: 0,
                Sbz1: 0,
                Control: 0,
                Owner: ptr::null_mut(),
                Group: ptr::null_mut(),
                Sacl: ptr::null_mut(),
                Dacl: ptr::null_mut(),
            },
        };
        let descriptor: PSECURITY_DESCRIPTOR = ptr::from_mut(&mut private.descriptor).cast();
        #[expect(
            unsafe_code,
            reason = "InitializeSecurityDescriptor has no safe binding"
        )]
        let initialized =
            unsafe { InitializeSecurityDescriptor(descriptor, SECURITY_DESCRIPTOR_REVISION) };
        succeeded(initialized)?;
        #[expect(
            unsafe_code,
            reason = "SetSecurityDescriptorDacl has no safe binding, and the list lives as long as the descriptor"
        )]
        let listed =
            unsafe { SetSecurityDescriptorDacl(descriptor, 1, private.acl.pointer().cast(), 0) };
        succeeded(listed)?;
        #[expect(
            unsafe_code,
            reason = "SetSecurityDescriptorControl has no safe binding"
        )]
        let protected = unsafe {
            SetSecurityDescriptorControl(descriptor, SE_DACL_PROTECTED, SE_DACL_PROTECTED)
        };
        succeeded(protected)?;
        Ok(private)
    }

    /// The descriptor, for a call that only reads it.
    const fn descriptor(&self) -> PSECURITY_DESCRIPTOR {
        ptr::from_ref(&self.descriptor).cast_mut().cast()
    }
}

fn wide(name: Name<'_>) -> Vec<u16> {
    name.as_str().encode_utf16().collect()
}

fn size_as_u32<T>() -> io::Result<u32> {
    u32::try_from(size_of::<T>()).map_err(|_outside| io::Error::other("a record is too long"))
}

fn succeeded(answer: windows_sys::core::BOOL) -> io::Result<()> {
    if answer == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn succeeded_nt(status: NTSTATUS) -> io::Result<()> {
    if status >= 0 {
        return Ok(());
    }
    #[expect(unsafe_code, reason = "RtlNtStatusToDosError has no safe binding")]
    let code = unsafe { RtlNtStatusToDosError(status) };
    match i32::try_from(code) {
        Ok(raw) => Err(io::Error::from_raw_os_error(raw)),
        Err(_outside) => Err(io::Error::other(format!(
            "the system answered {status:#010x}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::os::windows::io::AsRawHandle as _;
    use std::ptr;

    use super::records::{self, Buffer, Record};
    use super::{
        Create, FILE_CREATE, FILE_NON_DIRECTORY_FILE, FILE_WRITE_DATA, Kind, Name, create, open,
        open_entry, open_file, remove, status_at, succeeded, volume_verdict, wide,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FULL_DIR_INFO,
        FILE_WRITE_ATTRIBUTES,
    };
    use windows_sys::Win32::System::IO::DeviceIoControl;
    use windows_sys::Win32::System::SystemServices::IO_REPARSE_TAG_APPEXECLINK;

    #[test]
    fn bounded_directory_records_follow_the_windows_abi() {
        assert_eq!(std::mem::offset_of!(FILE_FULL_DIR_INFO, FileNameLength), 60);
        assert_eq!(std::mem::offset_of!(FILE_FULL_DIR_INFO, FileName), 68);
        assert_eq!(std::mem::offset_of!(super::ACCESS_ALLOWED_ACE, Mask), 4);
        assert_eq!(std::mem::offset_of!(super::ACCESS_ALLOWED_ACE, SidStart), 8);
    }

    #[test]
    fn the_attributes_and_the_tag_a_kind_is_read_from_follow_the_windows_headers() {
        assert_eq!(records::DIRECTORY_ATTRIBUTE, FILE_ATTRIBUTE_DIRECTORY);
        assert_eq!(
            records::REPARSE_POINT_ATTRIBUTE,
            FILE_ATTRIBUTE_REPARSE_POINT
        );
        assert_eq!(records::EXECUTION_ALIAS_TAG, IO_REPARSE_TAG_APPEXECLINK);
    }

    /// Makes `name` in `dir` an app execution alias naming a packaged app's executable, as the Store puts one on every user's path.
    fn execution_alias(dir: &File, name: Name<'_>) {
        const FSCTL_SET_REPARSE_POINT: u32 = 0x0009_00a4;
        let alias = create(
            dir,
            &wide(name),
            &Create {
                access: FILE_WRITE_DATA | FILE_WRITE_ATTRIBUTES,
                disposition: FILE_CREATE,
                options: FILE_NON_DIRECTORY_FILE,
                security: None,
            },
        )
        .expect("a new file to make an alias of");
        let mut data = 3_u32.to_le_bytes().to_vec();
        for field in [
            "Fabricated.Package_0123456789abc",
            "Fabricated.Package_0123456789abc!App",
            r"C:\Program Files\WindowsApps\Fabricated\app.exe",
            "0",
        ] {
            data.extend(field.encode_utf16().chain([0]).flat_map(u16::to_le_bytes));
        }
        let mut request = IO_REPARSE_TAG_APPEXECLINK.to_le_bytes().to_vec();
        request.extend_from_slice(
            &u16::try_from(data.len())
                .expect("an alias record fits its length field")
                .to_le_bytes(),
        );
        request.extend_from_slice(&[0, 0]);
        request.extend_from_slice(&data);
        let length = u32::try_from(request.len()).expect("an alias record fits a request");
        let buffer = Buffer::from_bytes(&request);
        let mut returned: u32 = 0;
        #[expect(unsafe_code, reason = "DeviceIoControl has no safe binding")]
        let set = unsafe {
            DeviceIoControl(
                alias.as_raw_handle(),
                FSCTL_SET_REPARSE_POINT,
                buffer.pointer(),
                length,
                ptr::null_mut(),
                0,
                &raw mut returned,
                ptr::null_mut(),
            )
        };
        succeeded(set).expect("an app execution alias any user may set on a file they made");
    }

    #[test]
    fn an_app_execution_alias_is_seen_as_one_and_never_opened_as_what_it_starts() {
        let temp = tempfile::tempdir().expect("tempdir");
        let dir = open(temp.path()).expect("the directory");
        let alias = Name::new("python.exe").expect("a component");
        execution_alias(&dir, alias);
        assert_eq!(
            status_at(&dir, alias)
                .expect("an alias is inspected without being followed")
                .map(|status| status.kind),
            Some(Kind::ExecutionAlias),
            "an alias is seen as what it is, not as a link to read"
        );
        let followed = File::open(temp.path().join("python.exe"));
        assert!(
            followed.is_err(),
            "no file open follows an alias to what it starts: {followed:?}"
        );
        assert!(
            matches!(open_entry(&dir, alias), Ok((_, Kind::ExecutionAlias))),
            "an entry opened without following it is the alias itself"
        );
        assert!(
            open_file(&dir, alias).is_err(),
            "and an alias is never handed out as a file to read"
        );
        remove(&dir, alias, false).expect("an alias is removed as an entry");
        assert_eq!(status_at(&dir, alias).expect("stat"), None);
    }
    use super::{Grant, Me, Privacy, Security, Sid, Volume, privacy_of};

    fn sid(last: u32) -> Sid {
        let bytes: Vec<u8> = [0x0000_0201_u32, 0x0500_0000, 21, last]
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect();
        Sid::copy(Record::new(&bytes)).expect("synthetic SID")
    }

    fn security(owner: u32, protected: bool, grants: Option<Vec<Grant>>) -> Security {
        Security {
            owner: sid(owner),
            protected,
            grants,
        }
    }

    fn full(last: u32) -> Grant {
        Grant::Allows {
            sid: sid(last),
            mask: windows_sys::Win32::Storage::FileSystem::FILE_ALL_ACCESS,
        }
    }

    #[test]
    fn owner_only_is_a_protected_list_of_the_trusted_that_gives_this_user_everything() {
        let me = Me {
            user: sid(42),
            owner: sid(544),
        };
        let trusted = [sid(42), sid(18), sid(544)];
        let exact = Some(vec![full(42), full(18), full(544)]);
        assert_eq!(
            privacy_of(&security(42, true, exact.clone()), &me, &trusted),
            Privacy::OwnerOnly
        );
        assert_eq!(
            privacy_of(&security(544, true, exact.clone()), &me, &trusted),
            Privacy::OwnerOnly,
            "what an elevated administrator makes is owned by the administrators, and is still ours"
        );
        assert_eq!(
            privacy_of(&security(41, true, exact.clone()), &me, &trusted),
            Privacy::ForeignOwner,
            "another user's directory is never one to tighten, however private its list"
        );
        assert_eq!(
            privacy_of(&security(42, false, exact), &me, &trusted),
            Privacy::Loose,
            "a list a parent can still add to is not private"
        );
        assert_eq!(
            privacy_of(&security(42, true, None), &me, &trusted),
            Privacy::Loose,
            "no list at all admits everybody"
        );
        assert_eq!(
            privacy_of(
                &security(42, true, Some(vec![full(42), full(18), full(1)])),
                &me,
                &trusted
            ),
            Privacy::Loose,
            "a grant to anyone else is loose"
        );
        assert_eq!(
            privacy_of(
                &security(42, true, Some(vec![full(42), Grant::Other])),
                &me,
                &trusted
            ),
            Privacy::Loose,
            "an entry of another kind is not one the private list has"
        );
        assert_eq!(
            privacy_of(
                &security(
                    42,
                    true,
                    Some(vec![Grant::Allows {
                        sid: sid(42),
                        mask: 1
                    }])
                ),
                &me,
                &trusted
            ),
            Privacy::Loose,
            "a list that does not give this user everything is one to tighten"
        );
    }

    #[test]
    fn a_volume_is_trusted_only_as_ntfs_or_refs_with_posix_semantics_and_a_refusal_names_it() {
        for (system, posix, kept) in [
            ("NTFS", true, true),
            ("ReFS", true, true),
            ("NTFS", false, false),
            ("FAT32", true, false),
            ("exFAT", false, false),
        ] {
            let verdict = volume_verdict(&Volume {
                system: system.to_owned(),
                posix,
            });
            assert_eq!(verdict.is_ok(), kept, "{system} with posix={posix}");
            if let Err(refused) = verdict {
                assert!(
                    refused.to_string().contains(system),
                    "a refusal names the file system it refused: {refused}"
                );
            }
        }
    }

    #[test]
    fn a_short_listing_record_is_no_word() {
        assert_eq!(Record::new(&[1, 0, 0, 0]).word(0).expect("whole word"), 1);
        assert_eq!(
            Record::new(&[1, 0, 0])
                .word(0)
                .expect_err("short word")
                .kind(),
            std::io::ErrorKind::InvalidData
        );
        assert_eq!(
            Record::new(&[1, 0, 0, 0])
                .word(usize::MAX)
                .expect_err("overflow")
                .kind(),
            std::io::ErrorKind::InvalidData
        );
    }
}
