//! Replace the running portable executable with a verified download, and hand
//! over to the new process.
//!
//! Windows lets a running executable be renamed but not overwritten, so the
//! swap renames `<name>.exe` to `<name>.old.exe` and moves the download into
//! its place. The next process deletes the old copy once this one has exited.

use std::ffi::{OsStr, OsString};
use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// `--updated-from <pid>`: the previous version started this process.
pub const UPDATED_FROM_FLAG: &str = "--updated-from";
/// `--updated-to <version>`: the release it installed. A build older than its
/// release must not be installed again, or every start would update.
pub const UPDATED_TO_FLAG: &str = "--updated-to";
/// Start minimized regardless of settings, as the window was before a restart.
pub const MINIMIZED_FLAG: &str = "--minimized";
/// A download older than this was abandoned (the app closed or crashed).
const STALE_DOWNLOAD: Duration = Duration::from_secs(60 * 60);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Paths {
    /// The running executable, under whatever name the user gave it.
    pub exe: PathBuf,
    /// `<name>.exe.download`: on the same volume, so the swap is a rename.
    pub staged: PathBuf,
    /// `<name>.old.exe`: the replaced version until it has exited.
    pub old: PathBuf,
}

impl Paths {
    pub fn for_exe(exe: &Path) -> Option<Self> {
        let directory = exe.parent()?;
        let file_name = exe.file_name()?;
        let stem = exe.file_stem()?;
        Some(Self {
            exe: exe.to_owned(),
            staged: directory.join(concat(file_name, ".download")),
            old: directory.join(concat(stem, ".old.exe")),
        })
    }

    pub fn current() -> Option<Self> {
        Self::for_exe(&std::env::current_exe().ok()?)
    }
}

fn concat(name: &OsStr, suffix: &str) -> OsString {
    let mut name = name.to_owned();
    name.push(suffix);
    name
}

/// Create the download file. Fails early in a read-only folder.
pub fn create_staged(paths: &Paths) -> io::Result<File> {
    OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&paths.staged)
}

/// Move the running executable aside and the verified download into place.
/// On failure the original executable is back under its own name.
pub fn swap(paths: &Paths) -> io::Result<()> {
    match fs::remove_file(&paths.old) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        // Still running from an earlier update: do not touch anything.
        Err(error) => return Err(error),
    }
    fs::rename(&paths.exe, &paths.old)?;
    if let Err(error) = fs::rename(&paths.staged, &paths.exe) {
        let _ = fs::rename(&paths.old, &paths.exe);
        return Err(error);
    }
    Ok(())
}

/// Remove what an update left behind: the replaced version, and a download
/// abandoned long enough ago that no other instance can still be writing it.
/// Only the two derived names are ever deleted.
pub fn clean_up(paths: &Paths, after_update: bool) {
    // The image of a process that has just exited can stay locked briefly.
    let attempts = if after_update { 10 } else { 1 };
    for attempt in 1..=attempts {
        match fs::remove_file(&paths.old) {
            Err(error) if error.kind() != io::ErrorKind::NotFound && attempt < attempts => {
                std::thread::sleep(Duration::from_millis(200));
            }
            _ => break,
        }
    }
    let stale = fs::metadata(&paths.staged)
        .and_then(|metadata| metadata.modified())
        .is_ok_and(|modified| modified.elapsed().is_ok_and(|age| age > STALE_DOWNLOAD));
    if stale {
        let _ = fs::remove_file(&paths.staged);
    }
}

fn flag_value<'a>(args: &'a [OsString], flag: &str) -> Option<&'a str> {
    let position = args.iter().position(|arg| arg == flag)?;
    args.get(position + 1)?.to_str()
}

/// The pid passed with `--updated-from`, if this process was started by an update.
pub fn previous_process(args: &[OsString]) -> Option<u32> {
    flag_value(args, UPDATED_FROM_FLAG)?.parse().ok()
}

/// The release version passed with `--updated-to`.
pub fn installed_release(args: &[OsString]) -> Option<String> {
    flag_value(args, UPDATED_TO_FLAG).map(str::to_owned)
}

/// Arguments for the new process: the original ones (so `--autostart` keeps
/// Start minimized working) without an earlier hand-over, plus this one.
pub fn restart_args(args: &[OsString], pid: u32, version: &str, minimized: bool) -> Vec<OsString> {
    let mut kept = Vec::new();
    let mut skip_value = false;
    for arg in args {
        if std::mem::take(&mut skip_value) {
            continue;
        }
        if arg == UPDATED_FROM_FLAG || arg == UPDATED_TO_FLAG {
            skip_value = true;
        } else if arg != MINIMIZED_FLAG {
            kept.push(arg.clone());
        }
    }
    kept.extend([
        UPDATED_FROM_FLAG.into(),
        pid.to_string().into(),
        UPDATED_TO_FLAG.into(),
        version.into(),
    ]);
    if minimized {
        kept.push(MINIMIZED_FLAG.into());
    }
    kept
}

/// Wait for the previous version to exit, so its files and WebView2 profile
/// are released before this process opens them. False on a timeout.
#[cfg(windows)]
pub fn wait_for_exit(pid: u32, timeout: Duration) -> bool {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
    use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};

    // SAFETY: a wait-only handle; an exited pid is reported as an error.
    let raw = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    if raw.is_null() {
        return true;
    }
    // SAFETY: OpenProcess returned a new owned handle.
    let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
    let millis = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX);
    // SAFETY: the handle is valid for the duration of the wait.
    unsafe { WaitForSingleObject(handle.as_raw_handle(), millis) == WAIT_OBJECT_0 }
}

#[cfg(not(windows))]
pub fn wait_for_exit(_pid: u32, _timeout: Duration) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    fn folder_with_exe() -> (tempfile::TempDir, Paths) {
        let directory = tempfile::tempdir().unwrap();
        let paths = Paths::for_exe(&directory.path().join("My Accounts.exe")).unwrap();
        fs::write(&paths.exe, "current").unwrap();
        (directory, paths)
    }

    #[test]
    fn derives_names_from_the_executable_the_user_runs() {
        let paths = Paths::for_exe(Path::new(r"D:\Games\My Accounts.exe")).unwrap();
        assert_eq!(paths.staged, Path::new(r"D:\Games\My Accounts.exe.download"));
        assert_eq!(paths.old, Path::new(r"D:\Games\My Accounts.old.exe"));
    }

    #[test]
    fn swap_installs_the_download_and_keeps_the_previous_copy_aside() {
        let (_directory, paths) = folder_with_exe();
        fs::write(&paths.old, "older").unwrap();
        fs::write(&paths.staged, "new").unwrap();
        swap(&paths).unwrap();
        assert_eq!(fs::read_to_string(&paths.exe).unwrap(), "new");
        assert_eq!(fs::read_to_string(&paths.old).unwrap(), "current");
        assert!(!paths.staged.exists());
    }

    #[test]
    fn failed_swap_restores_the_running_executable() {
        let (_directory, paths) = folder_with_exe();
        // No download: the second rename fails after the first succeeded.
        assert!(swap(&paths).is_err());
        assert_eq!(fs::read_to_string(&paths.exe).unwrap(), "current");
        assert!(!paths.old.exists());
    }

    #[test]
    #[cfg(windows)]
    fn swap_leaves_everything_in_place_while_an_old_copy_is_still_open() {
        use std::os::windows::fs::OpenOptionsExt;
        let (_directory, paths) = folder_with_exe();
        fs::write(&paths.old, "still running").unwrap();
        fs::write(&paths.staged, "new").unwrap();
        // Like a mapped executable image: no FILE_SHARE_DELETE.
        let _open = OpenOptions::new().read(true).share_mode(1).open(&paths.old).unwrap();
        assert!(swap(&paths).is_err());
        assert_eq!(fs::read_to_string(&paths.exe).unwrap(), "current");
        assert_eq!(fs::read_to_string(&paths.staged).unwrap(), "new");
    }

    #[test]
    fn clean_up_removes_only_leftovers_and_keeps_a_fresh_download() {
        let (directory, paths) = folder_with_exe();
        let unrelated = directory.path().join("notes.old.exe");
        fs::write(&unrelated, "keep").unwrap();
        fs::write(&paths.old, "replaced").unwrap();
        fs::write(&paths.staged, "another instance is downloading").unwrap();
        clean_up(&paths, false);
        assert!(!paths.old.exists());
        assert!(paths.staged.exists());
        assert!(paths.exe.exists() && unrelated.exists());

        let abandoned = std::time::SystemTime::now() - STALE_DOWNLOAD * 2;
        File::options().write(true).open(&paths.staged).unwrap().set_modified(abandoned).unwrap();
        clean_up(&paths, false);
        assert!(!paths.staged.exists());
    }

    #[test]
    fn restart_keeps_startup_arguments_and_replaces_an_earlier_hand_over() {
        let original = args(&["--autostart", UPDATED_FROM_FLAG, "41", UPDATED_TO_FLAG, "3.1.0", MINIMIZED_FLAG]);
        let restarted = restart_args(&original, 42, "3.2.0", false);
        assert_eq!(restarted, args(&["--autostart", UPDATED_FROM_FLAG, "42", UPDATED_TO_FLAG, "3.2.0"]));
        assert_eq!(
            restart_args(&[], 7, "3.2.0", true),
            args(&[UPDATED_FROM_FLAG, "7", UPDATED_TO_FLAG, "3.2.0", MINIMIZED_FLAG])
        );
        assert_eq!(previous_process(&restarted), Some(42));
        assert_eq!(installed_release(&restarted).as_deref(), Some("3.2.0"));
        assert_eq!(previous_process(&args(&[UPDATED_FROM_FLAG])), None);
        assert_eq!(previous_process(&args(&["--autostart"])), None);
        assert_eq!(installed_release(&args(&["--autostart"])), None);
    }

    #[test]
    #[cfg(windows)]
    fn waits_for_the_previous_process_to_exit() {
        let mut child = std::process::Command::new("cmd")
            .args(["/C", "ping -n 2 127.0.0.1 >NUL"])
            .spawn()
            .unwrap();
        assert!(!wait_for_exit(child.id(), Duration::from_millis(50)));
        assert!(wait_for_exit(child.id(), Duration::from_secs(10)));
        child.wait().unwrap();
        // Unknown pids (already exited and reaped) do not block startup.
        assert!(wait_for_exit(u32::MAX - 1, Duration::from_secs(10)));
    }
}
