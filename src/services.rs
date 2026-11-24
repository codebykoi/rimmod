use std::path::PathBuf;

pub(crate) mod assembly_metadata;
pub(crate) mod dependency_inference;
pub(crate) mod file_association;
pub(crate) mod fuzzy_search;
pub(crate) mod load_order;
pub(crate) mod log_analysis;
pub(crate) mod log_parser;
pub(crate) mod mod_deletion;
pub(crate) mod mod_loader;
pub(crate) mod mod_sorter;
pub(crate) mod mod_watcher;
pub(crate) mod modpack;
pub(crate) mod no_version_warning;
pub(crate) mod organization;
pub(crate) mod settings;
pub(crate) mod updater;
pub(crate) mod workshop;

/// Per-user directory where RimMod keeps its own data. Mirrors the platform
/// conventions already used for `organization.json`.
pub(crate) fn data_directory() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let app_data = std::env::var_os("APPDATA").map(PathBuf::from)?;

        Some(app_data.join("RimMod"))
    }

    #[cfg(target_os = "linux")]
    {
        let root = std::env::var_os("XDG_DATA_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .map(|home| home.join(".local").join("share"))
            })?;

        Some(root.join("rimmod"))
    }

    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME").map(PathBuf::from)?;

        Some(
            home.join("Library")
                .join("Application Support")
                .join("RimMod"),
        )
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        None
    }
}
