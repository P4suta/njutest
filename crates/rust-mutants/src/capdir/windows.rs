// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The capability directory on handle-relative NT opens, on a volume that deletes and renames the POSIX way (ADR 0037 decisions 3 to 7).

use std::ffi::c_void;
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
    FILE_RENAME_IGNORE_READONLY_ATTRIBUTE, FILE_RENAME_INFORMATION, FILE_RENAME_INFORMATION_0,
    FILE_RENAME_POSIX_SEMANTICS, FILE_RENAME_REPLACE_IF_EXISTS, FILE_SYNCHRONOUS_IO_NONALERT,
    FileRenameInformationEx, NTCREATEFILE_CREATE_DISPOSITION, NTCREATEFILE_CREATE_OPTIONS,
    NtCreateFile, NtSetInformationFile,
};
use windows_sys::Win32::Foundation::{
    ERROR_NO_MORE_FILES, ERROR_SHARING_VIOLATION, HANDLE, LocalFree, NTSTATUS,
    OBJ_CASE_INSENSITIVE, RtlNtStatusToDosError, UNICODE_STRING,
};
use windows_sys::Win32::Security::Authorization::{GetSecurityInfo, SE_FILE_OBJECT};
use windows_sys::Win32::Security::{
    ACCESS_ALLOWED_ACE, ACE_FLAGS, ACE_HEADER, ACL, ACL_REVISION, ACL_SIZE_INFORMATION,
    AclSizeInformation, AddAccessAllowedAceEx, CONTAINER_INHERIT_ACE, CreateWellKnownSid,
    DACL_SECURITY_INFORMATION, GetAce, GetAclInformation, GetLengthSid,
    GetSecurityDescriptorControl, GetTokenInformation, InitializeAcl, InitializeSecurityDescriptor,
    IsValidSid, OBJECT_INHERIT_ACE, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID,
    SE_DACL_PROTECTED, SECURITY_DESCRIPTOR, SECURITY_MAX_SID_SIZE, SetKernelObjectSecurity,
    SetSecurityDescriptorControl, SetSecurityDescriptorDacl, TOKEN_INFORMATION_CLASS, TOKEN_OWNER,
    TOKEN_QUERY, TOKEN_USER, TokenOwner, TokenUser, WELL_KNOWN_SID_TYPE,
    WinBuiltinAdministratorsSid, WinLocalSystemSid,
};
use windows_sys::Win32::Storage::FileSystem::{
    DELETE, FILE_ADD_FILE, FILE_ALL_ACCESS, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL,
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO, FILE_DISPOSITION_FLAG_DELETE,
    FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE, FILE_DISPOSITION_FLAG_POSIX_SEMANTICS,
    FILE_DISPOSITION_INFO_EX, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_FLAGS_AND_ATTRIBUTES, FILE_FULL_DIR_INFO, FILE_GENERIC_READ, FILE_GENERIC_WRITE,
    FILE_ID_128, FILE_ID_INFO, FILE_INFO_BY_HANDLE_CLASS, FILE_LIST_DIRECTORY,
    FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_MODE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    FILE_STANDARD_INFO, FILE_TRAVERSE, FILE_WRITE_DATA, FileAttributeTagInfo,
    FileDispositionInfoEx, FileFullDirectoryInfo, FileFullDirectoryRestartInfo, FileIdInfo,
    FileStandardInfo, FlushFileBuffers, GetFileInformationByHandleEx,
    GetVolumeInformationByHandleW, READ_CONTROL, SYNCHRONIZE, SetFileInformationByHandle,
    WRITE_DAC,
};
use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;
use windows_sys::Win32::System::SystemServices::{
    ACCESS_ALLOWED_ACE_TYPE, FILE_SUPPORTS_POSIX_UNLINK_RENAME, SECURITY_DESCRIPTOR_REVISION,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use super::{Identity, Kind, Name, Privacy, REMOVAL_DEPTH, Status};

/// What a held directory may do: list itself, be passed through, and show its attributes and its security.
const DIRECTORY_ACCESS: u32 =
    FILE_LIST_DIRECTORY | FILE_TRAVERSE | FILE_READ_ATTRIBUTES | READ_CONTROL | SYNCHRONIZE;

/// What a removal or a rename holds the entry with, which is nothing beyond taking its name away.
const MOVE_ACCESS: u32 = DELETE | FILE_READ_ATTRIBUTES | SYNCHRONIZE;

/// Every handle here lets another open, rename or remove the same entry, as a Unix descriptor never stops anyone.
const SHARE_ALL: FILE_SHARE_MODE = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;

/// How a directory named by a path is opened: as a directory, and as the object itself rather than what a link points at.
const DIRECTORY_BY_PATH: FILE_FLAGS_AND_ATTRIBUTES =
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
        .custom_flags(DIRECTORY_BY_PATH)
        .open(path)?;
    match kind_of(&directory)? {
        Kind::Directory => {}
        Kind::Other => return Err(link_refused()),
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
        Kind::Other | Kind::File => Err(link_refused()),
    }
}

pub(super) fn open_file(dir: &File, name: Name<'_>) -> io::Result<File> {
    let (opened, kind) = open_entry(dir, name)?;
    match kind {
        Kind::File | Kind::Directory => Ok(opened),
        Kind::Other => Err(link_refused()),
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
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    match kind_of(&opened)? {
        Kind::File | Kind::Directory => Ok(opened),
        Kind::Other => Err(link_refused()),
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
            (Kind::Directory, true) | (Kind::File | Kind::Other, false) => dispose(&entry),
            (Kind::Directory, false) => Err(io::Error::new(
                io::ErrorKind::IsADirectory,
                "a directory is removed as a directory",
            )),
            (Kind::File | Kind::Other, true) => Err(io::Error::new(
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
    let object_name = UNICODE_STRING {
        Length: length,
        MaximumLength: length,
        Buffer: name.as_ptr().cast_mut(),
    };
    let object = OBJECT_ATTRIBUTES {
        Length: size_as_u32::<OBJECT_ATTRIBUTES>()?,
        RootDirectory: parent.as_raw_handle(),
        ObjectName: &raw const object_name,
        Attributes: OBJ_CASE_INSENSITIVE,
        SecurityDescriptor: how.security.map_or(ptr::null(), |private| {
            private.descriptor().cast_const().cast()
        }),
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
    Ok(kind_of_attributes(tag.FileAttributes))
}

const fn kind_of_attributes(attributes: u32) -> Kind {
    if attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        Kind::Other
    } else if attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
        Kind::Directory
    } else {
        Kind::File
    }
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
    let named = system.split(|unit| *unit == 0).next().unwrap_or(&[]);
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
    let mut buffer = vec![0_u64; length.div_ceil(size_of::<u64>())];
    let header = FILE_RENAME_INFORMATION {
        Anonymous: FILE_RENAME_INFORMATION_0 { Flags: flags },
        RootDirectory: to_dir.as_raw_handle(),
        FileNameLength: u32::try_from(name_bytes)
            .map_err(|_outside| io::Error::other("a name is too long to rename to"))?,
        FileName: [0],
    };
    #[expect(
        unsafe_code,
        reason = "the buffer is eight-byte aligned and at least as long as the header"
    )]
    unsafe {
        buffer
            .as_mut_ptr()
            .cast::<FILE_RENAME_INFORMATION>()
            .write(header);
    }
    let spelled: Vec<u8> = target.iter().flat_map(|unit| unit.to_ne_bytes()).collect();
    #[expect(
        unsafe_code,
        reason = "the name ends within the buffer, which was sized as the header and the name together"
    )]
    unsafe {
        buffer
            .as_mut_ptr()
            .cast::<u8>()
            .add(name_at)
            .copy_from_nonoverlapping(spelled.as_ptr(), spelled.len());
    }
    let described = u32::try_from(length)
        .map_err(|_outside| io::Error::other("a rename cannot be described"))?;
    let mut status_block = IO_STATUS_BLOCK::default();
    #[expect(unsafe_code, reason = "NtSetInformationFile has no safe binding")]
    let status = unsafe {
        NtSetInformationFile(
            held.as_raw_handle(),
            &raw mut status_block,
            buffer.as_ptr().cast(),
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
    const BATCH_WORDS: usize = 8192;
    let scan = reopen(dir, FILE_LIST_DIRECTORY, FILE_DIRECTORY_FILE)?;
    let mut buffer = vec![0_u64; BATCH_WORDS];
    let capacity = u32::try_from(size_of_val(buffer.as_slice()))
        .map_err(|_outside| io::Error::other("a listing buffer is too long"))?;
    let mut class = FileFullDirectoryRestartInfo;
    let mut names = Vec::new();
    loop {
        #[expect(
            unsafe_code,
            reason = "GetFileInformationByHandleEx has no safe binding"
        )]
        let read = unsafe {
            GetFileInformationByHandleEx(
                scan.as_raw_handle(),
                class,
                buffer.as_mut_ptr().cast(),
                capacity,
            )
        };
        if read == 0 {
            let error = io::Error::last_os_error();
            return match (error.raw_os_error(), i32::try_from(ERROR_NO_MORE_FILES)) {
                (Some(raw), Ok(ended)) if raw == ended => Ok(names),
                (Some(_) | None, Ok(_) | Err(_)) => Err(error),
            };
        }
        class = FileFullDirectoryInfo;
        let bytes: Vec<u8> = buffer.iter().flat_map(|word| word.to_ne_bytes()).collect();
        names.extend(
            batch(&bytes)?
                .into_iter()
                .filter(|name| !matches!(name.as_slice(), [46] | [46, 46])),
        );
    }
}

/// The names one batch of `FILE_FULL_DIR_INFO` records holds, read as bytes so no record is taken for more than it says it is.
fn batch(bytes: &[u8]) -> io::Result<Vec<Vec<u16>>> {
    let length_at = std::mem::offset_of!(FILE_FULL_DIR_INFO, FileNameLength);
    let name_at = std::mem::offset_of!(FILE_FULL_DIR_INFO, FileName);
    let broken = || {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "a directory listing record runs past what the system returned",
        )
    };
    let mut names = Vec::new();
    let mut at = 0_usize;
    loop {
        let next = word_at(bytes, at).ok_or_else(broken)?;
        let length =
            word_at(bytes, at.checked_add(length_at).ok_or_else(broken)?).ok_or_else(broken)?;
        let start = at.checked_add(name_at).ok_or_else(broken)?;
        let end = usize::try_from(length)
            .map_err(|_outside| broken())?
            .checked_add(start)
            .ok_or_else(broken)?;
        let (units, uneven) = bytes.get(start..end).ok_or_else(broken)?.as_chunks::<2>();
        if !uneven.is_empty() {
            return Err(broken());
        }
        names.push(units.iter().map(|pair| u16::from_ne_bytes(*pair)).collect());
        if next == 0 {
            return Ok(names);
        }
        at = at
            .checked_add(usize::try_from(next).map_err(|_outside| broken())?)
            .ok_or_else(broken)?;
    }
}

fn word_at(bytes: &[u8], at: usize) -> Option<u32> {
    let end = at.checked_add(4)?;
    let four = <[u8; 4]>::try_from(bytes.get(at..end)?);
    match four {
        Ok(four) => Some(u32::from_ne_bytes(four)),
        Err(_short) => None,
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
struct Sid(Vec<u32>);

impl Sid {
    /// Copies the identifier `raw` points at.
    fn copy(raw: PSID) -> io::Result<Self> {
        #[expect(unsafe_code, reason = "IsValidSid has no safe binding")]
        let valid = unsafe { IsValidSid(raw) };
        succeeded(valid)
            .map_err(|_invalid| io::Error::new(io::ErrorKind::InvalidData, "a SID is not valid"))?;
        #[expect(unsafe_code, reason = "GetLengthSid has no safe binding")]
        let length = unsafe { GetLengthSid(raw) };
        let bytes = usize::try_from(length)
            .map_err(|_outside| io::Error::other("a SID is too long to hold"))?;
        #[expect(
            unsafe_code,
            reason = "a valid SID is exactly as long as GetLengthSid says, and nothing writes it while it is copied"
        )]
        let held = unsafe { std::slice::from_raw_parts(raw.cast::<u8>().cast_const(), bytes) };
        let (words, rest) = held.as_chunks::<4>();
        if !rest.is_empty() {
            return Err(io::Error::other("a SID is not whole words"));
        }
        Ok(Self(
            words.iter().map(|word| u32::from_ne_bytes(*word)).collect(),
        ))
    }

    /// The identifier the system gives the principal `kind` names.
    fn well_known(kind: WELL_KNOWN_SID_TYPE) -> io::Result<Self> {
        let mut size = SECURITY_MAX_SID_SIZE;
        let words = usize::try_from(SECURITY_MAX_SID_SIZE)
            .map_err(|_outside| io::Error::other("a SID is too long to hold"))?
            .div_ceil(4);
        let mut held = vec![0_u32; words];
        #[expect(unsafe_code, reason = "CreateWellKnownSid has no safe binding")]
        let made = unsafe {
            CreateWellKnownSid(
                kind,
                ptr::null_mut(),
                held.as_mut_ptr().cast(),
                &raw mut size,
            )
        };
        succeeded(made)?;
        let used = usize::try_from(size)
            .map_err(|_outside| io::Error::other("a SID is too long to hold"))?
            .div_ceil(4);
        held.truncate(used);
        Ok(Self(held))
    }

    const fn as_psid(&self) -> PSID {
        self.0.as_ptr().cast_mut().cast()
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
        let user = token_sid(&token, TokenUser, |buffer| {
            #[expect(
                unsafe_code,
                reason = "GetTokenInformation filled the pointer-aligned buffer with a TOKEN_USER"
            )]
            let user = unsafe { buffer.cast::<TOKEN_USER>().read() };
            user.User.Sid
        })?;
        let owner = token_sid(&token, TokenOwner, |buffer| {
            #[expect(
                unsafe_code,
                reason = "GetTokenInformation filled the pointer-aligned buffer with a TOKEN_OWNER"
            )]
            let owner = unsafe { buffer.cast::<TOKEN_OWNER>().read() };
            owner.Owner
        })?;
        Ok(Self { user, owner })
    }
}

/// The identifier the token's `class` names, read by `sid` out of the buffer the system filled.
fn token_sid<F>(token: &OwnedHandle, class: TOKEN_INFORMATION_CLASS, sid: F) -> io::Result<Sid>
where
    F: FnOnce(*const usize) -> PSID,
{
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
    let words = usize::try_from(needed)
        .map_err(|_outside| io::Error::other("the process token holds too much"))?
        .div_ceil(size_of::<usize>());
    let mut buffer = vec![0_usize; words];
    let capacity = size_of_val(buffer.as_slice())
        .try_into()
        .map_err(|_outside| io::Error::other("the process token holds too much"))?;
    #[expect(unsafe_code, reason = "GetTokenInformation has no safe binding")]
    let read = unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            class,
            buffer.as_mut_ptr().cast(),
            capacity,
            &raw mut needed,
        )
    };
    succeeded(read)?;
    Sid::copy(sid(buffer.as_ptr()))
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
        let mut owner: PSID = ptr::null_mut();
        let mut dacl: *mut ACL = ptr::null_mut();
        let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
        #[expect(unsafe_code, reason = "GetSecurityInfo has no safe binding")]
        let asked = unsafe {
            GetSecurityInfo(
                dir.as_raw_handle(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                &raw mut owner,
                ptr::null_mut(),
                &raw mut dacl,
                ptr::null_mut(),
                &raw mut descriptor,
            )
        };
        if asked != 0 {
            return Err(match i32::try_from(asked) {
                Ok(raw) => io::Error::from_raw_os_error(raw),
                Err(_outside) => io::Error::other("the security of a directory could not be read"),
            });
        }
        let held = Described(descriptor);
        let mut control: u16 = 0;
        let mut revision: u32 = 0;
        #[expect(
            unsafe_code,
            reason = "GetSecurityDescriptorControl has no safe binding"
        )]
        let controlled =
            unsafe { GetSecurityDescriptorControl(held.0, &raw mut control, &raw mut revision) };
        succeeded(controlled)?;
        let security = Self {
            owner: Sid::copy(owner)?,
            protected: control & SE_DACL_PROTECTED != 0,
            grants: if dacl.is_null() {
                None
            } else {
                Some(grants(dacl)?)
            },
        };
        drop(held);
        Ok(security)
    }
}

/// A security descriptor the system allocated, freed when it is dropped.
struct Described(PSECURITY_DESCRIPTOR);

impl Drop for Described {
    fn drop(&mut self) {
        #[expect(
            unsafe_code,
            reason = "GetSecurityInfo allocated the descriptor for this owner to free once"
        )]
        let freed = unsafe { LocalFree(self.0) };
        if !freed.is_null() {
            std::process::abort();
        }
    }
}

fn grants(dacl: *const ACL) -> io::Result<Vec<Grant>> {
    let mut size = ACL_SIZE_INFORMATION {
        AceCount: 0,
        AclBytesInUse: 0,
        AclBytesFree: 0,
    };
    let length = size_as_u32::<ACL_SIZE_INFORMATION>()?;
    #[expect(unsafe_code, reason = "GetAclInformation has no safe binding")]
    let asked = unsafe {
        GetAclInformation(
            dacl,
            ptr::from_mut(&mut size).cast(),
            length,
            AclSizeInformation,
        )
    };
    succeeded(asked)?;
    let sid_at = std::mem::offset_of!(ACCESS_ALLOWED_ACE, SidStart);
    let mut found = Vec::new();
    for index in 0..size.AceCount {
        let mut ace: *mut c_void = ptr::null_mut();
        #[expect(unsafe_code, reason = "GetAce has no safe binding")]
        let got = unsafe { GetAce(dacl, index, &raw mut ace) };
        succeeded(got)?;
        #[expect(
            unsafe_code,
            reason = "every access control entry begins with its header"
        )]
        let header = unsafe { ace.cast::<ACE_HEADER>().read_unaligned() };
        if u32::from(header.AceType) != ACCESS_ALLOWED_ACE_TYPE {
            found.push(Grant::Other);
            continue;
        }
        #[expect(
            unsafe_code,
            reason = "an entry whose header says it allows access is laid out as ACCESS_ALLOWED_ACE"
        )]
        let allowed = unsafe { ace.cast::<ACCESS_ALLOWED_ACE>().read_unaligned() };
        #[expect(
            unsafe_code,
            reason = "the SID an allowing entry grants begins at its SidStart field"
        )]
        let raw = unsafe { ace.cast::<u8>().add(sid_at) };
        found.push(Grant::Allows {
            sid: Sid::copy(raw.cast())?,
            mask: allowed.Mask,
        });
    }
    Ok(found)
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
    acl: Vec<u32>,
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
                .and_then(|with| with.checked_add(size_of_val(sid.0.as_slice())))
                .ok_or_else(|| io::Error::other("an access control list cannot be sized"))?;
        }
        let mut acl = vec![0_u32; length.div_ceil(4)];
        let capacity = size_of_val(acl.as_slice())
            .try_into()
            .map_err(|_outside| io::Error::other("an access control list is too long"))?;
        #[expect(unsafe_code, reason = "InitializeAcl has no safe binding")]
        let initialized = unsafe { InitializeAcl(acl.as_mut_ptr().cast(), capacity, ACL_REVISION) };
        succeeded(initialized)?;
        for sid in &trusted {
            #[expect(unsafe_code, reason = "AddAccessAllowedAceEx has no safe binding")]
            let added = unsafe {
                AddAccessAllowedAceEx(
                    acl.as_mut_ptr().cast(),
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
            unsafe { SetSecurityDescriptorDacl(descriptor, 1, private.acl.as_ptr().cast(), 0) };
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
    use super::{Grant, Kind, Me, Privacy, Security, Sid, Volume, kind_of_attributes, privacy_of};
    use super::{volume_verdict, word_at};

    fn sid(last: u32) -> Sid {
        Sid(vec![0x0000_0501, 0x0500_0000, 21, last])
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
    fn a_reparse_point_is_never_what_it_points_at() {
        assert_eq!(kind_of_attributes(0x10 | 0x400), Kind::Other);
        assert_eq!(kind_of_attributes(0x20 | 0x400), Kind::Other);
        assert_eq!(kind_of_attributes(0x10), Kind::Directory);
        assert_eq!(kind_of_attributes(0x20), Kind::File);
    }

    #[test]
    fn a_short_listing_record_is_no_word() {
        assert_eq!(word_at(&[1, 0, 0, 0], 0), Some(1));
        assert_eq!(word_at(&[1, 0, 0], 0), None);
        assert_eq!(word_at(&[1, 0, 0, 0], usize::MAX), None);
    }
}
