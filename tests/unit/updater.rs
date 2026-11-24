use std::ffi::OsString;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use super::*;

fn sample_release_json(tag: &str, draft: bool, prerelease: bool, download_prefix: &str) -> String {
    format!(
        r#"{{
            "tag_name": "{tag}",
            "draft": {draft},
            "prerelease": {prerelease},
            "html_url": "https://github.com/codebykoi/rimmod/releases/tag/{tag}",
            "assets": [
                {{
                    "name": "rimmod-{tag}-windows-x86_64.zip",
                    "browser_download_url": "{download_prefix}/releases/download/{tag}/rimmod-{tag}-windows-x86_64.zip"
                }},
                {{
                    "name": "rimmod-{tag}-linux-x86_64.tar.gz",
                    "browser_download_url": "{download_prefix}/releases/download/{tag}/rimmod-{tag}-linux-x86_64.tar.gz"
                }}
            ]
        }}"#
    )
}

fn unique_temp_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let dir = std::env::temp_dir().join(format!(
        "rimmod-updater-test-{label}-{}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).expect("create test temp directory");
    dir
}

#[test]
fn parses_and_orders_semver_versions() {
    let v030 = parse_version("0.3.0").unwrap();
    let v031 = parse_version("v0.3.1").unwrap();
    let v010 = parse_version("V0.10.0+build.42").unwrap();
    let v030_rc = parse_version("v0.3.0-rc.1").unwrap();

    assert!(v030_rc < v030);
    assert!(v030 < v031);
    assert!(v031 < v010);

    assert!(parse_version("").is_none());
    assert!(parse_version("0.3").is_none());
    assert!(parse_version("0.3.0.1").is_none());
    assert!(parse_version("0.3.0-").is_none());
    assert!(parse_version("abc").is_none());
}

#[test]
fn selects_platform_asset_for_newer_release() {
    let json = sample_release_json(
        "v0.4.0",
        false,
        false,
        "https://github.com/codebykoi/rimmod",
    );

    let windows = parse_release_update(&json, "0.3.0", "windows", "x86_64")
        .unwrap()
        .expect("expected windows update");
    assert_eq!(windows.version, "0.4.0");
    assert_eq!(windows.tag, "v0.4.0");
    assert_eq!(windows.asset_name, "rimmod-v0.4.0-windows-x86_64.zip");
    assert_eq!(
        windows.download_url,
        "https://github.com/codebykoi/rimmod/releases/download/v0.4.0/rimmod-v0.4.0-windows-x86_64.zip"
    );

    let linux = parse_release_update(&json, "0.3.0", "linux", "x86_64")
        .unwrap()
        .expect("expected linux update");
    assert_eq!(linux.asset_name, "rimmod-v0.4.0-linux-x86_64.tar.gz");

    assert!(
        parse_release_update(&json, "0.3.0", "macos", "aarch64")
            .unwrap()
            .is_none()
    );
}

#[test]
fn ignores_same_older_draft_prerelease_and_untrusted_releases() {
    let same = sample_release_json(
        "v0.3.0",
        false,
        false,
        "https://github.com/codebykoi/rimmod",
    );
    assert!(
        parse_release_update(&same, "0.3.0", "windows", "x86_64")
            .unwrap()
            .is_none()
    );

    let older = sample_release_json(
        "v0.2.0",
        false,
        false,
        "https://github.com/codebykoi/rimmod",
    );
    assert!(
        parse_release_update(&older, "0.3.0", "windows", "x86_64")
            .unwrap()
            .is_none()
    );

    let draft = sample_release_json("v0.4.0", true, false, "https://github.com/codebykoi/rimmod");
    assert!(
        parse_release_update(&draft, "0.3.0", "windows", "x86_64")
            .unwrap()
            .is_none()
    );

    let prerelease =
        sample_release_json("v0.4.0", false, true, "https://github.com/codebykoi/rimmod");
    assert!(
        parse_release_update(&prerelease, "0.3.0", "windows", "x86_64")
            .unwrap()
            .is_none()
    );

    let prerelease_tag = sample_release_json(
        "v0.4.0-beta.1",
        false,
        false,
        "https://github.com/codebykoi/rimmod",
    );
    assert!(
        parse_release_update(&prerelease_tag, "0.3.0", "windows", "x86_64")
            .unwrap()
            .is_none()
    );

    let untrusted = sample_release_json("v0.4.0", false, false, "https://example.com/evil");
    assert!(
        parse_release_update(&untrusted, "0.3.0", "windows", "x86_64")
            .unwrap()
            .is_none()
    );
}

#[test]
fn parses_normal_and_updater_cli_arguments() {
    let normal = parse_startup_args([OsString::from("rimmod.exe")]);
    assert_eq!(normal, StartupMode::Normal(StartupNotice::default()));

    let with_notice = parse_startup_args([
        OsString::from("rimmod.exe"),
        OsString::from("--cleanup-updater"),
        OsString::from("/tmp/rimmod-updater-42"),
        OsString::from("--update-failed"),
        OsString::from("connection reset"),
    ]);
    assert_eq!(
        with_notice,
        StartupMode::Normal(StartupNotice {
            cleanup_updater: Some(PathBuf::from("/tmp/rimmod-updater-42")),
            update_error: Some("connection reset".to_owned()),
        })
    );

    let apply = parse_startup_args([
        OsString::from("rimmod-updater-42.exe"),
        OsString::from("--apply-update"),
        OsString::from("--pid"),
        OsString::from("42"),
        OsString::from("--target"),
        OsString::from("C:\\Tools\\rimmod.exe"),
        OsString::from("--url"),
        OsString::from("https://github.com/codebykoi/rimmod/releases/download/v0.4.0/rimmod.zip"),
        OsString::from("--asset"),
        OsString::from("rimmod-v0.4.0-windows-x86_64.zip"),
    ]);
    assert_eq!(
        apply,
        StartupMode::ApplyUpdate(ApplyUpdateArgs {
            parent_pid: 42,
            target_exe: PathBuf::from("C:\\Tools\\rimmod.exe"),
            download_url: "https://github.com/codebykoi/rimmod/releases/download/v0.4.0/rimmod.zip"
                .to_owned(),
            asset_name: "rimmod-v0.4.0-windows-x86_64.zip".to_owned(),
        })
    );

    let incomplete = parse_startup_args([
        OsString::from("rimmod.exe"),
        OsString::from("--apply-update"),
        OsString::from("--pid"),
        OsString::from("42"),
    ]);
    assert_eq!(incomplete, StartupMode::Normal(StartupNotice::default()));
}

#[test]
fn extracts_expected_binary_from_zip_and_skips_unsafe_entries() {
    let dir = unique_temp_dir("zip");
    let archive_path = dir.join("release.zip");
    let output_path = dir.join("extracted.exe");

    {
        let file = File::create(&archive_path).unwrap();
        let mut writer = ZipWriter::new(file);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

        writer.start_file("../rimmod.exe", options).unwrap();
        writer.write_all(b"malicious").unwrap();

        writer.start_file("README.txt", options).unwrap();
        writer.write_all(b"notes").unwrap();

        writer.start_file("rimmod.exe", options).unwrap();
        writer.write_all(b"fresh-binary-payload").unwrap();

        writer.finish().unwrap();
    }

    extract_binary_from_zip(&archive_path, "rimmod.exe", &output_path).unwrap();
    assert_eq!(fs::read(&output_path).unwrap(), b"fresh-binary-payload");

    let missing_result = extract_binary_from_zip(&archive_path, "rimmod", &output_path);
    assert!(missing_result.is_err());

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn replaces_existing_binary_cleanly() {
    let dir = unique_temp_dir("replace");
    let target = dir.join("rimmod.exe");
    let staged = dir.join("staged-rimmod.exe");

    fs::write(&target, b"old-version").unwrap();
    fs::write(&staged, b"new-version").unwrap();

    replace_binary(&target, &staged).unwrap();

    assert_eq!(fs::read(&target).unwrap(), b"new-version");
    assert!(!backup_path_for(&target).exists());

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn backup_path_appends_old_suffix() {
    assert_eq!(
        backup_path_for(Path::new("rimmod.exe")),
        PathBuf::from("rimmod.exe.old")
    );
    assert_eq!(
        backup_path_for(Path::new("rimmod")),
        PathBuf::from("rimmod.old")
    );
}

#[test]
fn validates_cleanup_paths_strictly() {
    let valid = std::env::temp_dir().join("rimmod-updater-1234.exe");
    assert!(is_safe_cleanup_path(&valid));

    let wrong_name = std::env::temp_dir().join("not-rimmod.exe");
    assert!(!is_safe_cleanup_path(&wrong_name));

    let wrong_dir = PathBuf::from("rimmod-updater-1234.exe");
    assert!(!is_safe_cleanup_path(&wrong_dir));
}
