//! Windows atomic replacement; report commit failure before replacing data.
#![cfg(windows)]
use std::{io, path::Path};

pub(crate) fn commit_replace(temp: &Path, target: &Path) -> io::Result<()> {
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };
    // Canonical parent/source paths carry the verbatim Windows prefix and
    // therefore also work beyond MAX_PATH without a process-wide setting.
    let source_path = std::fs::canonicalize(temp)?;
    let _parent = target
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "replacement has no parent"))?;
    let source_parent = source_path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "temporary replacement has no parent",
        )
    })?;
    // AppContainer can redirect a newly-created temp file from the logical
    // Roaming path into LocalCache. When the target already exists, verify
    // its physical parent; when it does not, the temp's physical parent is
    // the only authoritative destination for this same-directory commit.
    if target.exists() {
        let existing_parent = std::fs::canonicalize(target)?
            .parent()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "target has no parent"))?
            .to_owned();
        if !crate::platform::windows::paths::same_path(source_parent, &existing_parent)
            && !crate::platform::windows::paths::is_appcontainer_redirect(
                &existing_parent,
                source_parent,
            )
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "temporary replacement must share the target directory",
            ));
        }
    }
    let name = target.file_name().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "replacement has no filename")
    })?;
    let source = crate::platform::windows::paths::wide_path(&source_path)?;
    let destination = crate::platform::windows::paths::wide_path(&source_parent.join(name))?;
    // The caller has fsynced and closed the same-directory temporary file.
    // WRITE_THROUGH requests the strongest MoveFileEx completion available;
    // it is not a promise against faulty hardware or loss of drive caches.
    if unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, io::Write, os::windows::fs::OpenOptionsExt, path::PathBuf};

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("atrio-windows-fs-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn temp(&self, contents: &[u8]) -> PathBuf {
            let path = self.0.join(format!("{}.tmp", uuid::Uuid::new_v4()));
            let mut file = fs::File::create(&path).unwrap();
            file.write_all(contents).unwrap();
            file.sync_all().unwrap();
            path
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn replacement_updates_existing_target_and_consumes_temp() {
        let fixture = Fixture::new();
        let target = fixture.0.join("中文.md");
        fs::write(&target, b"before").unwrap();
        let temp = fixture.temp(b"after");
        commit_replace(&temp, &target).unwrap();
        assert_eq!(fs::read(target).unwrap(), b"after");
        assert!(!temp.exists());
    }

    #[test]
    fn denied_replacement_preserves_original_and_leaves_temp_for_cleanup() {
        let fixture = Fixture::new();
        let target = fixture.0.join("locked.md");
        fs::write(&target, b"before").unwrap();
        let _exclusive = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&target)
            .unwrap();
        let temp = fixture.temp(b"after");
        assert!(commit_replace(&temp, &target).is_err());
        drop(_exclusive);
        assert_eq!(fs::read(target).unwrap(), b"before");
        assert!(temp.exists());
    }

    #[test]
    fn aliases_streams_devices_and_namespace_paths_are_rejected() {
        for path in [
            "C:/file",
            "C:file",
            "//host/share",
            "\\\\?\\C:\\file",
            "a:secret",
            "a/../b",
            "a/./b",
            "a//b",
            "a.",
            "a ",
            "NUL",
            "con.txt",
            "COM1.txt",
            "LPT9",
            "COM¹",
            "CONIN$",
            "a\u{001f}",
        ] {
            assert!(
                crate::platform::windows::paths::validate_relative_path(path).is_err(),
                "accepted {path:?}"
            );
        }
        for path in [
            "notes/中文.md",
            "ordinary file.md",
            "COM10.txt",
            "concrete.md",
        ] {
            crate::platform::windows::paths::validate_relative_path(path).unwrap();
        }
    }

    #[test]
    fn appcontainer_verbatim_and_case_aliases_stay_within_the_workspace() {
        let root = PathBuf::from(r"\\?\C:\Users\Admin\AppData\Local\Packages\Atrio\Workspace");
        let child =
            PathBuf::from(r"C:\Users\admin\AppData\Local\Packages\Atrio\Workspace\.workspace");
        let sibling =
            PathBuf::from(r"C:\Users\admin\AppData\Local\Packages\Atrio\Workspace-copy\.workspace");
        assert!(crate::platform::windows::paths::is_within(&root, &child));
        assert!(!crate::platform::windows::paths::is_within(&root, &sibling));
        let logical =
            PathBuf::from(r"C:\Users\admin\AppData\Roaming\dev.pixel.workspace\Workspace");
        let redirected = PathBuf::from(
            r"\\?\C:\Users\admin\AppData\Local\Packages\Atrio\LocalCache\Roaming\dev.pixel.workspace\Workspace\.workspace\workspace.sqlite3",
        );
        assert!(crate::platform::windows::paths::is_within(
            &logical,
            &redirected
        ));
    }

    #[test]
    fn appcontainer_roaming_redirect_is_the_only_cross_parent_alias() {
        let logical = PathBuf::from(r"C:\Users\Admin\AppData\Roaming\dev.pixel.workspace");
        let physical = PathBuf::from(
            r"\\?\C:\Users\admin\AppData\Local\Packages\OpenAI.Codex_2p2nqsd0c76g0\LocalCache\Roaming\dev.pixel.workspace",
        );
        let other = PathBuf::from(r"C:\Users\admin\AppData\Local\Temp\dev.pixel.workspace");
        assert!(crate::platform::windows::paths::is_appcontainer_redirect(
            &logical, &physical
        ));
        assert!(!crate::platform::windows::paths::is_appcontainer_redirect(
            &logical, &other
        ));
    }

    #[test]
    fn replacement_supports_long_paths_and_read_only_failure_preserves_old_data() {
        let fixture = Fixture::new();
        let long_parent = fixture
            .0
            .join("a".repeat(100))
            .join("b".repeat(100))
            .join("c".repeat(100));
        fs::create_dir_all(&long_parent).unwrap();
        let target = long_parent.join("document.md");
        let temp = long_parent.join("replacement.tmp");
        fs::write(&target, b"before").unwrap();
        fs::write(&temp, b"after").unwrap();
        fs::OpenOptions::new()
            .write(true)
            .open(&temp)
            .unwrap()
            .sync_all()
            .unwrap();
        let mut permissions = fs::metadata(&target).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&target, permissions).unwrap();
        let outcome = commit_replace(&temp, &target);
        let mut permissions = fs::metadata(&target).unwrap().permissions();
        permissions.set_readonly(false);
        fs::set_permissions(&target, permissions).unwrap();
        assert!(outcome.is_err());
        assert_eq!(fs::read(&target).unwrap(), b"before");
        commit_replace(&temp, &target).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"after");
    }

    #[test]
    fn directory_identity_survives_case_alias_but_changes_after_replacement() {
        let fixture = Fixture::new();
        let root = fixture.0.join("workspace");
        fs::create_dir(&root).unwrap();
        let first = crate::platform::windows::paths::directory_identity(&root).unwrap();
        assert_eq!(
            first,
            crate::platform::windows::paths::directory_identity(&fixture.0.join("WORKSPACE"))
                .unwrap()
        );
        fs::rename(&root, fixture.0.join("previous")).unwrap();
        fs::create_dir(&root).unwrap();
        assert_ne!(
            first,
            crate::platform::windows::paths::directory_identity(&root).unwrap()
        );
    }
}
