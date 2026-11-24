use std::path::PathBuf;

pub(crate) mod bottom_panel;
pub(crate) mod icons;
pub(crate) mod logs;
pub(crate) mod mod_filters;
pub(crate) mod mod_info;
pub(crate) mod mod_list;
pub(crate) mod modpacks;
pub(crate) mod organize;
pub(crate) mod settings_window;
pub(crate) mod shortcuts;
pub(crate) mod theme;
pub(crate) mod top_panel;
pub(crate) mod window_resize;
pub(crate) mod workshop;

pub(crate) enum OpenTarget {
    Folder(PathBuf),
    File(PathBuf),
    Url(String),
}

impl OpenTarget {
    pub(crate) fn description(&self) -> String {
        match self {
            Self::Folder(path) => format!("folder {}", path.display()),
            Self::File(path) => format!("file {}", path.display()),
            Self::Url(url) => format!("URL {url}"),
        }
    }
}
