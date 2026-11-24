use crate::models::PackageId;

use super::*;

#[cfg(target_os = "windows")]
mod windows_only {
    use std::fs;
    use std::io;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    static NEXT_TEST_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    struct TestDirectory {
        path: PathBuf,
    }

    impl TestDirectory {
        fn new() -> io::Result<Self> {
            let unique_number = NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "rimmod-launch-test-{}-{unique_number}",
                std::process::id()
            ));

            fs::create_dir(&path)?;

            Ok(Self { path })
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    /// RimWorld 1.7 dropped the Unity launchers and ships `RimWorld.exe`.
    #[test]
    fn rimworld_1_7_executable_is_found() {
        let directory = TestDirectory::new().expect("test directory");
        let executable = directory.path.join("RimWorld.exe");
        fs::write(&executable, []).expect("write executable");

        assert_eq!(
            find_rimworld_executable(&directory.path).expect("executable"),
            executable
        );
    }

    #[test]
    fn missing_executable_is_reported() {
        let directory = TestDirectory::new().expect("test directory");

        assert!(find_rimworld_executable(&directory.path).is_err());
    }
}

fn package_ids(raw_ids: &[&str]) -> Vec<PackageId> {
    raw_ids
        .iter()
        .map(|raw_id| PackageId::new(raw_id).expect("valid package ID"))
        .collect()
}

#[test]
fn mod_argument_joins_package_ids() {
    let package_ids = package_ids(&["ludeon.rimworld.biotech", "test.mod"]);

    assert_eq!(
        mod_argument(&package_ids),
        Some("-mod=ludeon.rimworld.biotech,test.mod".to_owned())
    );
}

#[test]
fn mod_argument_is_omitted_without_active_mods() {
    assert_eq!(mod_argument(&[]), None);
}

#[test]
fn mod_argument_requires_rimworld_1_7() {
    assert!(supports_mod_argument(Some("1.7.5014 rev1195")));
    assert!(!supports_mod_argument(Some("1.6.4850 rev1234")));
    assert!(!supports_mod_argument(None));
}
