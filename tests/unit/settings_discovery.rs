use super::*;

#[test]
fn windows_config_path_uses_the_user_profile() {
    let user_profile = Path::new(r"C:\Users\RimModTest");

    let config_path = windows_config_path(user_profile);

    assert_eq!(
        config_path,
        user_profile
            .join("AppData")
            .join("LocalLow")
            .join("Ludeon Studios")
            .join("RimWorld by Ludeon Studios")
            .join("Config")
    );
}

#[test]
fn linux_config_path_prefers_xdg_config_home() {
    let config_path = linux_config_path(
        Some(PathBuf::from("/custom/config")),
        Some(PathBuf::from("/home/rimmod")),
    );

    assert_eq!(
        config_path,
        Some(
            PathBuf::from("/custom/config")
                .join("unity3d")
                .join("Ludeon Studios")
                .join("RimWorld by Ludeon Studios")
                .join("Config")
        )
    );
}

#[test]
fn linux_config_path_falls_back_to_home() {
    let config_path = linux_config_path(None, Some(PathBuf::from("/home/rimmod")));

    assert_eq!(
        config_path,
        Some(
            PathBuf::from("/home/rimmod")
                .join(".config")
                .join("unity3d")
                .join("Ludeon Studios")
                .join("RimWorld by Ludeon Studios")
                .join("Config")
        )
    );
}
