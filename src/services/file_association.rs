//! Registers the `.rimpack` file extension icon per-user so file managers
//! display RimMod's icon on exported modpacks without requiring admin rights.
//!
//! Intentionally registers only the file type name and icon (no double-click
//! `open` command).

#[cfg(any(windows, test))]
use std::borrow::Cow;
#[cfg(any(windows, test))]
use std::path::Path;

#[cfg(windows)]
const PROG_ID: &str = "RimMod.Modpack";
#[cfg(windows)]
const FILE_TYPE_LABEL: &str = "RimMod Modpack";

/// Best-effort per-user registration of the `.rimpack` file icon.
///
/// Updates the registered path automatically if the portable executable moved,
/// and only notifies the OS shell when something actually changed.
pub(crate) fn register_rimpack_icon() {
    #[cfg(windows)]
    {
        if let Ok(exe_path) = std::env::current_exe() {
            let _ = register_windows_icon(&exe_path);
        }
    }

    #[cfg(target_os = "linux")]
    {
        let _ = register_linux_icon();
    }
}

#[cfg(any(windows, test))]
fn format_default_icon_value(exe_path: &Path) -> String {
    let raw = exe_path.to_string_lossy();
    let clean = strip_verbatim_prefix(&raw);
    format!("\"{clean}\",0")
}

#[cfg(any(windows, test))]
fn strip_verbatim_prefix(path: &str) -> Cow<'_, str> {
    if let Some(unc) = path.strip_prefix(r"\\?\UNC\") {
        Cow::Owned(format!(r"\\{unc}"))
    } else if let Some(local) = path.strip_prefix(r"\\?\") {
        Cow::Borrowed(local)
    } else {
        Cow::Borrowed(path)
    }
}

#[cfg(windows)]
fn register_windows_icon(exe_path: &Path) -> Result<(), u32> {
    use std::ptr;

    use windows_sys::Win32::UI::Shell::{SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify};

    let icon_value = format_default_icon_value(exe_path);
    let mut changed = false;

    changed |= ensure_hkcu_default_value(r"Software\Classes\.rimpack", PROG_ID)?;
    changed |= ensure_hkcu_default_value(r"Software\Classes\RimMod.Modpack", FILE_TYPE_LABEL)?;
    changed |=
        ensure_hkcu_default_value(r"Software\Classes\RimMod.Modpack\DefaultIcon", &icon_value)?;

    if changed {
        // SAFETY: `SHChangeNotify` with `SHCNE_ASSOCCHANGED` and null item
        // pointers asks Explorer to refresh file-type icon associations.
        unsafe {
            SHChangeNotify(
                SHCNE_ASSOCCHANGED as i32,
                SHCNF_IDLIST,
                ptr::null(),
                ptr::null(),
            );
        }
    }

    Ok(())
}

#[cfg(windows)]
fn ensure_hkcu_default_value(subkey: &str, value: &str) -> Result<bool, u32> {
    use std::ptr;

    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_SZ, RRF_RT_REG_SZ, RegCloseKey,
        RegCreateKeyExW, RegGetValueW, RegSetValueExW,
    };

    let subkey_wide = to_wide_null(subkey);
    let value_wide = to_wide_null(value);

    if read_hkcu_default_value(&subkey_wide).as_deref() == Some(value) {
        return Ok(false);
    }

    let mut key: HKEY = ptr::null_mut();
    // SAFETY: `subkey_wide` is NUL-terminated UTF-16 and `key` is a valid out-pointer.
    let create_status = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            subkey_wide.as_ptr(),
            0,
            ptr::null(),
            0,
            KEY_SET_VALUE,
            ptr::null(),
            &mut key,
            ptr::null_mut(),
        )
    };
    if create_status != ERROR_SUCCESS {
        return Err(create_status);
    }

    let byte_len = u32::try_from(value_wide.len() * std::mem::size_of::<u16>()).unwrap_or(u32::MAX);
    // SAFETY: `key` is an open HKEY, `value_wide` is NUL-terminated UTF-16 with `byte_len` bytes.
    let set_status = unsafe {
        RegSetValueExW(
            key,
            ptr::null(),
            0,
            REG_SZ,
            value_wide.as_ptr().cast::<u8>(),
            byte_len,
        )
    };
    // SAFETY: `key` was opened by `RegCreateKeyExW` above.
    unsafe {
        RegCloseKey(key);
    }

    if set_status != ERROR_SUCCESS {
        return Err(set_status);
    }

    return Ok(true);

    fn read_hkcu_default_value(subkey_wide: &[u16]) -> Option<String> {
        let mut byte_len: u32 = 0;
        // SAFETY: Querying required byte size with null data buffer.
        let size_status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                subkey_wide.as_ptr(),
                ptr::null(),
                RRF_RT_REG_SZ,
                ptr::null_mut(),
                ptr::null_mut(),
                &mut byte_len,
            )
        };
        if size_status != ERROR_SUCCESS || byte_len < 2 || !byte_len.is_multiple_of(2) {
            return None;
        }

        let mut buffer = vec![0u16; (byte_len / 2) as usize];
        // SAFETY: `buffer` has `byte_len` writable bytes.
        let read_status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                subkey_wide.as_ptr(),
                ptr::null(),
                RRF_RT_REG_SZ,
                ptr::null_mut(),
                buffer.as_mut_ptr().cast(),
                &mut byte_len,
            )
        };
        if read_status != ERROR_SUCCESS {
            return None;
        }

        while buffer.last() == Some(&0) {
            buffer.pop();
        }
        String::from_utf16(&buffer).ok()
    }
}

#[cfg(windows)]
fn to_wide_null(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(any(target_os = "linux", test))]
fn linux_mime_package_xml() -> &'static str {
    r#"<?xml version="1.0" encoding="UTF-8"?>
<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info">
  <mime-type type="application/x-rimmod-modpack">
    <comment>RimMod Modpack</comment>
    <sub-class-of type="application/zip"/>
    <glob pattern="*.rimpack"/>
    <icon name="rimmod-rimpack"/>
  </mime-type>
</mime-info>
"#
}

#[cfg(target_os = "linux")]
fn register_linux_icon() -> std::io::Result<()> {
    use std::path::PathBuf;
    use std::process::Command;

    const APP_ICON_SVG: &[u8] = include_bytes!("../../assets/icons/app_icon.svg");
    const APP_ICON_PNG: &[u8] = include_bytes!("../../assets/icons/app_icon.png");

    let data_home = std::env::var_os("XDG_DATA_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|home| home.join(".local").join("share"))
        })
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no XDG data dir"))?;

    let mime_dir = data_home.join("mime");
    let mime_file = mime_dir.join("packages").join("rimmod-rimpack.xml");
    let hicolor_dir = data_home.join("icons").join("hicolor");
    let svg_icon = hicolor_dir
        .join("scalable")
        .join("mimetypes")
        .join("rimmod-rimpack.svg");
    let png_icon = hicolor_dir
        .join("256x256")
        .join("mimetypes")
        .join("rimmod-rimpack.png");

    let mime_changed = write_if_changed(&mime_file, linux_mime_package_xml().as_bytes())?;
    let mut icon_changed = false;
    icon_changed |= write_if_changed(&svg_icon, APP_ICON_SVG)?;
    icon_changed |= write_if_changed(&png_icon, APP_ICON_PNG)?;

    if mime_changed {
        let _ = Command::new("update-mime-database").arg(&mime_dir).status();
    }
    if icon_changed {
        let _ = Command::new("gtk-update-icon-cache")
            .args(["-f", "-t"])
            .arg(&hicolor_dir)
            .status();
    }

    Ok(())
}

#[cfg(target_os = "linux")]
fn write_if_changed(path: &std::path::Path, contents: &[u8]) -> std::io::Result<bool> {
    use std::fs;

    if let Ok(existing) = fs::read(path)
        && existing == contents
    {
        return Ok(false);
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, contents)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_icon_value_quotes_path_and_strips_verbatim_prefix() {
        assert_eq!(
            format_default_icon_value(Path::new(r"C:\Games\RimMod\rimmod.exe")),
            "\"C:\\Games\\RimMod\\rimmod.exe\",0"
        );
        assert_eq!(
            format_default_icon_value(Path::new(r"\\?\C:\Program Files\RimMod\rimmod.exe")),
            "\"C:\\Program Files\\RimMod\\rimmod.exe\",0"
        );
        assert_eq!(
            format_default_icon_value(Path::new(r"\\?\UNC\server\share\rimmod.exe")),
            "\"\\\\server\\share\\rimmod.exe\",0"
        );
    }

    #[test]
    fn linux_mime_package_declares_rimpack_glob_and_icon() {
        let xml = linux_mime_package_xml();
        assert!(xml.contains(r#"<glob pattern="*.rimpack"/>"#));
        assert!(xml.contains(r#"<icon name="rimmod-rimpack"/>"#));
        assert!(xml.contains("<comment>RimMod Modpack</comment>"));
    }
}
