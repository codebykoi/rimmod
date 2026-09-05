use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Permanently remove a mod folder after verifying that it is a direct child
/// of one of the configured, non-official mod roots.
pub(crate) fn remove_mod_folder(mod_folder: &Path, allowed_roots: &[PathBuf]) -> io::Result<()> {
    let metadata = fs::symlink_metadata(mod_folder)?;

    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "mod path is not a normal directory",
        ));
    }

    let canonical_mod_folder = fs::canonicalize(mod_folder)?;
    let is_allowed = allowed_roots.iter().any(|root| {
        fs::canonicalize(root).is_ok_and(|canonical_root| {
            canonical_mod_folder.parent() == Some(canonical_root.as_path())
        })
    });

    if !is_allowed {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "mod folder is outside the configured local and Workshop directories",
        ));
    }

    fs::remove_dir_all(canonical_mod_folder)
}

#[cfg(test)]
#[path = "../../tests/unit/mod_deletion.rs"]
mod tests;
