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
            "rimmod-mod-deletion-test-{}-{unique_number}",
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

#[test]
fn removes_a_direct_child_of_an_allowed_mod_root() -> io::Result<()> {
    let test_directory = TestDirectory::new()?;
    let mods_root = test_directory.path.join("Mods");
    let mod_folder = mods_root.join("ExampleMod");
    fs::create_dir_all(mod_folder.join("About"))?;

    remove_mod_folder(&mod_folder, std::slice::from_ref(&mods_root))?;

    assert!(!mod_folder.exists());
    assert!(mods_root.is_dir());
    Ok(())
}

#[test]
fn refuses_to_remove_a_folder_outside_allowed_mod_roots() -> io::Result<()> {
    let test_directory = TestDirectory::new()?;
    let mods_root = test_directory.path.join("Mods");
    let outside_folder = test_directory.path.join("OutsideMod");
    fs::create_dir_all(&mods_root)?;
    fs::create_dir_all(&outside_folder)?;

    let error = remove_mod_folder(&outside_folder, &[mods_root])
        .expect_err("an outside folder must not be deleted");

    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert!(outside_folder.is_dir());
    Ok(())
}

#[test]
fn refuses_to_remove_an_allowed_root_itself() -> io::Result<()> {
    let test_directory = TestDirectory::new()?;
    let mods_root = test_directory.path.join("Mods");
    fs::create_dir_all(&mods_root)?;

    let error = remove_mod_folder(&mods_root, std::slice::from_ref(&mods_root))
        .expect_err("the mod root itself must not be deleted");

    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert!(mods_root.is_dir());
    Ok(())
}

#[test]
fn refuses_to_remove_a_nested_non_mod_folder() -> io::Result<()> {
    let test_directory = TestDirectory::new()?;
    let mods_root = test_directory.path.join("Mods");
    let mod_folder = mods_root.join("ExampleMod");
    let nested_folder = mod_folder.join("About");
    fs::create_dir_all(&nested_folder)?;

    let error = remove_mod_folder(&nested_folder, &[mods_root])
        .expect_err("only direct children of a mod root may be deleted");

    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert!(nested_folder.is_dir());
    Ok(())
}
