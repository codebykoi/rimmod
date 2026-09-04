use std::path::PathBuf;

pub(crate) mod bottom_panel;
pub(crate) mod icons;
pub(crate) mod mod_info;
pub(crate) mod mod_list;
pub(crate) mod settings_window;
pub(crate) mod top_panel;

pub(crate) enum OpenTarget {
    Folder(PathBuf),
    Url(String),
}

impl OpenTarget {
    pub(crate) fn description(&self) -> String {
        match self {
            Self::Folder(path) => format!("folder {}", path.display()),
            Self::Url(url) => format!("URL {url}"),
        }
    }
}
