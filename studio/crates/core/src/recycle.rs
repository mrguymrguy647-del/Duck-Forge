//! Throwing files away the safe way: to the Recycle Bin (the Trash on
//! Linux and macOS), so they can be put back. Nothing the Studio deletes
//! is ever deleted for good.

use std::path::{Path, PathBuf};

/// Where thrown-away files go.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum RecycleBin {
    /// The system's: Windows' Recycle Bin, or the Trash.
    #[default]
    System,
    /// A folder (the tests use one, so they can look at what arrived).
    Folder(PathBuf),
}

impl RecycleBin {
    /// Moves each file or folder to the Recycle Bin. If one can't go
    /// there, it says so, and it isn't deleted.
    pub fn throw_away(&self, paths: &[PathBuf]) -> Result<(), String> {
        let paths: Vec<PathBuf> = paths
            .iter()
            .filter(|path| path.exists())
            .map(|path| std::path::absolute(path).unwrap_or_else(|_| path.clone()))
            .collect();
        if paths.is_empty() {
            return Ok(());
        }
        match self {
            RecycleBin::Folder(folder) => {
                std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
                for path in &paths {
                    let to = free_name(folder, path);
                    if let Err(error) = std::fs::rename(path, &to) {
                        // Another drive (Windows can't move a file there by
                        // renaming it): a copy, then the file goes.
                        if !path.is_file() {
                            return Err(format!("{}: {error}", path.display()));
                        }
                        std::fs::copy(path, &to)
                            .and_then(|_| std::fs::remove_file(path))
                            .map_err(|e| format!("{}: {e}", path.display()))?;
                    }
                }
                Ok(())
            }
            RecycleBin::System => system(&paths),
        }?;
        match paths.iter().find(|path| path.exists()) {
            Some(left) => Err(format!(
                "{} couldn't be moved to the Recycle Bin, so it's still there",
                left.display()
            )),
            None => Ok(()),
        }
    }
}

/// `name`, or `name 2`, `name 3`... so nothing already in `folder` is
/// replaced.
fn free_name(folder: &Path, path: &Path) -> PathBuf {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let (stem, dot_ext) = match name.rfind('.') {
        Some(i) if i > 0 && path.is_file() => (&name[..i], &name[i..]),
        _ => (&*name, ""),
    };
    let mut candidate = folder.join(&*name);
    let mut n = 2;
    while candidate.exists() {
        candidate = folder.join(format!("{stem} {n}{dot_ext}"));
        n += 1;
    }
    candidate
}

/// Windows: the shell's own delete, with "allow undo", which is what the
/// Recycle Bin is. If a file can't go there (a drive without one), Windows
/// asks before deleting it for good, and nothing is deleted unless the
/// person says yes.
#[cfg(windows)]
fn system(paths: &[PathBuf]) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::{
        FO_DELETE, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT,
        FOF_WANTNUKEWARNING, SHFILEOPSTRUCTW, SHFileOperationW,
    };
    // Every path, each ending with a 0, and one more 0 at the end.
    let mut from: Vec<u16> = Vec::new();
    for path in paths {
        from.extend(path.as_os_str().encode_wide());
        from.push(0);
    }
    from.push(0);
    let mut operation = SHFILEOPSTRUCTW {
        hwnd: std::ptr::null_mut(),
        wFunc: FO_DELETE,
        pFrom: from.as_ptr(),
        pTo: std::ptr::null(),
        fFlags: (FOF_ALLOWUNDO
            | FOF_NOCONFIRMATION
            | FOF_SILENT
            | FOF_NOERRORUI
            | FOF_WANTNUKEWARNING) as u16,
        fAnyOperationsAborted: 0,
        hNameMappings: std::ptr::null_mut(),
        lpszProgressTitle: std::ptr::null(),
    };
    // SAFETY: `from` is a list of 0-ended wide strings ending with an extra
    // 0, as SHFileOperationW asks, and it outlives the call.
    let result = unsafe { SHFileOperationW(&mut operation) };
    if result != 0 {
        return Err(format!("Windows said {result:#x}"));
    }
    if operation.fAnyOperationsAborted != 0 {
        return Err(String::from("it was stopped before it finished"));
    }
    Ok(())
}

/// macOS: the Trash in the home folder.
#[cfg(target_os = "macos")]
fn system(paths: &[PathBuf]) -> Result<(), String> {
    let home = std::env::var_os("HOME").ok_or("no home folder")?;
    let trash = PathBuf::from(home).join(".Trash");
    for path in paths {
        let to = free_name(&trash, path);
        std::fs::rename(path, &to)
            .map_err(|e| format!("{} couldn't go to the Trash: {e}", path.display()))?;
    }
    Ok(())
}

/// Linux and the like: the Trash as the desktop's file managers keep it
/// (freedesktop.org's layout), so it can be put back from there.
#[cfg(not(any(windows, target_os = "macos")))]
fn system(paths: &[PathBuf]) -> Result<(), String> {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| Some(PathBuf::from(std::env::var_os("HOME")?).join(".local/share")))
        .ok_or("no home folder")?;
    trash_into(&data, paths)
}

/// Into the Trash kept in `data` (`~/.local/share`, normally).
#[cfg(not(any(windows, target_os = "macos")))]
fn trash_into(data: &Path, paths: &[PathBuf]) -> Result<(), String> {
    let (files, info) = (data.join("Trash/files"), data.join("Trash/info"));
    std::fs::create_dir_all(&files).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&info).map_err(|e| e.to_string())?;
    for path in paths {
        let to = free_name(&files, path);
        let name = to
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let note = info.join(format!("{name}.trashinfo"));
        std::fs::write(
            &note,
            format!(
                "[Trash Info]\nPath={}\nDeletionDate={}\n",
                path.display(),
                now_as_text()
            ),
        )
        .map_err(|e| e.to_string())?;
        if let Err(error) = std::fs::rename(path, &to) {
            let _ = std::fs::remove_file(&note);
            return Err(format!(
                "{} couldn't go to the Trash: {error}",
                path.display()
            ));
        }
    }
    Ok(())
}

/// The time now, as the Trash writes it: 2026-11-12T08:30:00 (UTC).
#[cfg(not(any(windows, target_os = "macos")))]
fn now_as_text() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs()) as i64;
    let (days, rest) = (seconds.div_euclid(86_400), seconds.rem_euclid(86_400));
    // Days since 1970 to a date (Howard Hinnant's civil_from_days).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_folder::TestFolder;

    #[test]
    fn things_thrown_away_arrive_in_the_bin_and_nothing_is_replaced() {
        let test = TestFolder::new("recycle");
        let bin = RecycleBin::Folder(test.join("bin"));
        std::fs::create_dir_all(test.join("game/assets")).unwrap();
        std::fs::write(test.join("game/assets/duck.png"), "picture").unwrap();
        std::fs::write(test.join("duck.png"), "another").unwrap();
        bin.throw_away(&[test.join("game"), test.join("duck.png")])
            .unwrap();
        assert!(!test.join("game").exists() && !test.join("duck.png").exists());
        assert!(test.join("bin/game/assets/duck.png").is_file());
        // A second duck.png doesn't replace the first.
        std::fs::write(test.join("duck.png"), "third").unwrap();
        bin.throw_away(&[test.join("duck.png")]).unwrap();
        assert_eq!(
            std::fs::read_to_string(test.join("bin/duck.png")).unwrap(),
            "another"
        );
        assert_eq!(
            std::fs::read_to_string(test.join("bin/duck 2.png")).unwrap(),
            "third"
        );
        // Something that isn't there is no problem.
        bin.throw_away(&[test.join("nothing")]).unwrap();
    }

    /// Windows' real Recycle Bin: the shell's own delete with undo.
    #[cfg(windows)]
    #[test]
    fn the_recycle_bin_takes_a_file() {
        let test = TestFolder::new("recycle-system");
        let file = test.join("thrown away.txt");
        std::fs::write(&file, "bye").unwrap();
        RecycleBin::System
            .throw_away(std::slice::from_ref(&file))
            .unwrap();
        assert!(!file.exists());
    }

    /// The Trash as Linux's file managers keep it, in a folder of the
    /// test's own instead of the home folder.
    #[cfg(not(any(windows, target_os = "macos")))]
    #[test]
    fn the_trash_takes_a_file_and_says_where_it_was() {
        let test = TestFolder::new("recycle-trash");
        let file = test.join("thrown away.txt");
        std::fs::write(&file, "bye").unwrap();
        trash_into(&test.join("data"), std::slice::from_ref(&file)).unwrap();
        assert!(!file.exists());
        let note = std::fs::read_to_string(test.join("data/Trash/info/thrown away.txt.trashinfo"))
            .unwrap();
        assert!(
            note.contains("[Trash Info]") && note.contains("DeletionDate=20"),
            "{note}"
        );
        assert!(note.contains(&format!("Path={}", file.display())), "{note}");
        assert!(test.join("data/Trash/files/thrown away.txt").is_file());
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    #[test]
    fn the_date_is_written_as_the_trash_wants() {
        let now = now_as_text();
        assert_eq!(now.len(), 19, "{now}");
        assert_eq!(&now[4..5], "-");
        assert_eq!(&now[10..11], "T");
    }
}
