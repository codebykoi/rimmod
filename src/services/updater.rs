//! Non-invasive GitHub Releases self-updater.
//!
//! Startup checks the latest stable release in the background. When the user
//! clicks the title-bar update icon, RimMod copies itself to a temporary helper
//! executable, launches that helper detached with `--apply-update`, and exits
//! cleanly so settings and window state persist. The helper downloads and
//! extracts the new binary, waits for the old process to exit, swaps the
//! executable in place, and relaunches RimMod cleanly.

use std::cmp::Ordering;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde::Deserialize;
use zip::ZipArchive;

const GITHUB_LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/codebykoi/rimmod/releases/latest";
const GITHUB_RELEASE_DOWNLOAD_PREFIX: &str = "https://github.com/codebykoi/rimmod/";
const GITHUB_RELEASES_FALLBACK_URL: &str = "https://github.com/codebykoi/rimmod/releases/latest";

const CHECK_TIMEOUT: Duration = Duration::from_secs(15);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(180);
const PARENT_EXIT_TIMEOUT: Duration = Duration::from_secs(30);
const REPLACE_RETRY_ATTEMPTS: usize = 30;
const REPLACE_RETRY_DELAY: Duration = Duration::from_millis(150);
const CLEANUP_RETRY_ATTEMPTS: usize = 25;
const CLEANUP_RETRY_DELAY: Duration = Duration::from_millis(200);

const UPDATER_FILE_PREFIX: &str = "rimmod-updater-";
const ARG_APPLY_UPDATE: &str = "--apply-update";
const ARG_PID: &str = "--pid";
const ARG_TARGET: &str = "--target";
const ARG_URL: &str = "--url";
const ARG_ASSET: &str = "--asset";
const ARG_CLEANUP_UPDATER: &str = "--cleanup-updater";
const ARG_UPDATE_FAILED: &str = "--update-failed";

pub(crate) const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SemVer {
    pub(crate) major: u64,
    pub(crate) minor: u64,
    pub(crate) patch: u64,
    pub(crate) prerelease: Option<String>,
}

impl Ord for SemVer {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(other.major, other.minor, other.patch))
            .then_with(|| match (&self.prerelease, &other.prerelease) {
                (None, None) => Ordering::Equal,
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(left), Some(right)) => left.cmp(right),
            })
    }
}

impl PartialOrd for SemVer {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub(crate) fn parse_version(raw: &str) -> Option<SemVer> {
    let trimmed = raw.trim();
    let without_prefix = trimmed
        .strip_prefix('v')
        .or_else(|| trimmed.strip_prefix('V'))
        .unwrap_or(trimmed);

    // Strip optional SemVer build metadata (`+...`).
    let without_build = without_prefix.split('+').next().unwrap_or(without_prefix);

    let (core, prerelease) = match without_build.split_once('-') {
        Some((core, pre)) if !pre.is_empty() => (core, Some(pre.to_owned())),
        Some(_) => return None,
        None => (without_build, None),
    };

    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;

    if parts.next().is_some() {
        return None;
    }

    Some(SemVer {
        major,
        minor,
        patch,
        prerelease,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AvailableUpdate {
    pub(crate) version: String,
    pub(crate) tag: String,
    pub(crate) asset_name: String,
    pub(crate) download_url: String,
    pub(crate) release_url: String,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct StartupNotice {
    pub(crate) cleanup_updater: Option<PathBuf>,
    pub(crate) update_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ApplyUpdateArgs {
    pub(crate) parent_pid: u32,
    pub(crate) target_exe: PathBuf,
    pub(crate) download_url: String,
    pub(crate) asset_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StartupMode {
    Normal(StartupNotice),
    ApplyUpdate(ApplyUpdateArgs),
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    assets: Vec<GitHubAsset>,
}

#[derive(Debug, Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

pub(crate) fn platform_asset_suffix(os: &str, arch: &str) -> Option<&'static str> {
    match (os, arch) {
        ("windows", "x86_64") => Some("-windows-x86_64.zip"),
        ("linux", "x86_64") => Some("-linux-x86_64.tar.gz"),
        _ => None,
    }
}

pub(crate) fn parse_startup_args<I>(args: I) -> StartupMode
where
    I: IntoIterator<Item = OsString>,
{
    let mut iter = args.into_iter();
    // Skip argv[0] (the executable path).
    let _ = iter.next();

    let mut apply_update = false;
    let mut parent_pid: Option<u32> = None;
    let mut target_exe: Option<PathBuf> = None;
    let mut download_url: Option<String> = None;
    let mut asset_name: Option<String> = None;
    let mut notice = StartupNotice::default();

    while let Some(arg) = iter.next() {
        let Some(flag) = arg.to_str() else {
            continue;
        };

        match flag {
            ARG_APPLY_UPDATE => apply_update = true,
            ARG_PID => {
                parent_pid = iter
                    .next()
                    .and_then(|value| value.into_string().ok())
                    .and_then(|value| value.parse::<u32>().ok());
            }
            ARG_TARGET => {
                target_exe = iter.next().map(PathBuf::from);
            }
            ARG_URL => {
                download_url = iter.next().and_then(|value| value.into_string().ok());
            }
            ARG_ASSET => {
                asset_name = iter.next().and_then(|value| value.into_string().ok());
            }
            ARG_CLEANUP_UPDATER => {
                notice.cleanup_updater = iter.next().map(PathBuf::from);
            }
            ARG_UPDATE_FAILED => {
                notice.update_error = iter
                    .next()
                    .map(|value| value.to_string_lossy().into_owned())
                    .filter(|value| !value.trim().is_empty());
            }
            _ => {}
        }
    }

    if apply_update
        && let (Some(parent_pid), Some(target_exe), Some(download_url), Some(asset_name)) =
            (parent_pid, target_exe, download_url, asset_name)
    {
        return StartupMode::ApplyUpdate(ApplyUpdateArgs {
            parent_pid,
            target_exe,
            download_url,
            asset_name,
        });
    }

    StartupMode::Normal(notice)
}

fn http_client(timeout: Duration) -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(timeout)
        .user_agent(concat!("RimMod/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|error| format!("could not create the HTTP client: {error}"))
}

pub(crate) fn check_for_update() -> Result<Option<AvailableUpdate>, String> {
    if platform_asset_suffix(std::env::consts::OS, std::env::consts::ARCH).is_none() {
        return Ok(None);
    }

    let client = http_client(CHECK_TIMEOUT)?;
    let response = client
        .get(GITHUB_LATEST_RELEASE_URL)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .map_err(|error| format!("update check failed: {error}"))?;

    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }

    if !response.status().is_success() {
        return Err(format!(
            "GitHub returned an unexpected status ({})",
            response.status()
        ));
    }

    let body = response
        .text()
        .map_err(|error| format!("could not read release metadata: {error}"))?;

    parse_release_update(
        &body,
        CURRENT_VERSION,
        std::env::consts::OS,
        std::env::consts::ARCH,
    )
}

pub(crate) fn parse_release_update(
    json: &str,
    current_version: &str,
    os: &str,
    arch: &str,
) -> Result<Option<AvailableUpdate>, String> {
    let release: GitHubRelease = serde_json::from_str(json)
        .map_err(|error| format!("could not parse release metadata: {error}"))?;

    Ok(find_update_in_release(&release, current_version, os, arch))
}

fn find_update_in_release(
    release: &GitHubRelease,
    current_version: &str,
    os: &str,
    arch: &str,
) -> Option<AvailableUpdate> {
    if release.draft || release.prerelease {
        return None;
    }

    let current = parse_version(current_version)?;
    let remote = parse_version(&release.tag_name)?;

    if remote.prerelease.is_some() || remote <= current {
        return None;
    }

    let suffix = platform_asset_suffix(os, arch)?;
    let asset = release.assets.iter().find(|asset| {
        asset.name.ends_with(suffix)
            && asset
                .browser_download_url
                .starts_with(GITHUB_RELEASE_DOWNLOAD_PREFIX)
    })?;

    let release_url = if release.html_url.starts_with("https://") {
        release.html_url.clone()
    } else {
        GITHUB_RELEASES_FALLBACK_URL.to_owned()
    };

    Some(AvailableUpdate {
        version: format!("{}.{}.{}", remote.major, remote.minor, remote.patch),
        tag: release.tag_name.clone(),
        asset_name: asset.name.clone(),
        download_url: asset.browser_download_url.clone(),
        release_url,
    })
}

pub(crate) fn backup_path_for(target_exe: &Path) -> PathBuf {
    let mut os = target_exe.as_os_str().to_os_string();
    os.push(".old");
    PathBuf::from(os)
}

fn verify_directory_writable(directory: &Path) -> Result<(), String> {
    let probe = directory.join(format!(".rimmod-update-probe-{}", std::process::id()));

    match File::create(&probe) {
        Ok(file) => {
            drop(file);
            let _ = fs::remove_file(&probe);
            Ok(())
        }
        Err(error) => Err(format!(
            "cannot update in place because {} is not writable ({error})",
            directory.display()
        )),
    }
}

/// Copy the running binary to a temporary updater helper and launch it detached.
///
/// The caller should close the GUI immediately when this returns `Ok(())`.
pub(crate) fn launch_updater(update: &AvailableUpdate) -> Result<(), String> {
    let target_exe = std::env::current_exe()
        .map_err(|error| format!("could not locate the running executable: {error}"))?;
    let target_dir = target_exe
        .parent()
        .ok_or_else(|| "could not determine the application folder".to_owned())?;

    verify_directory_writable(target_dir)?;

    let pid = std::process::id();
    let helper_name = format!("{UPDATER_FILE_PREFIX}{pid}{}", std::env::consts::EXE_SUFFIX);
    let helper_path = std::env::temp_dir().join(helper_name);

    fs::copy(&target_exe, &helper_path)
        .map_err(|error| format!("could not stage the updater helper: {error}"))?;
    set_executable_permissions(&helper_path)?;

    let mut command = Command::new(&helper_path);
    command
        .arg(ARG_APPLY_UPDATE)
        .arg(ARG_PID)
        .arg(pid.to_string())
        .arg(ARG_TARGET)
        .arg(&target_exe)
        .arg(ARG_URL)
        .arg(&update.download_url)
        .arg(ARG_ASSET)
        .arg(&update.asset_name)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    configure_detached(&mut command);

    if let Err(error) = command.spawn() {
        let _ = fs::remove_file(&helper_path);
        return Err(format!("could not start the updater helper: {error}"));
    }

    Ok(())
}

/// Headless entry point executed by the detached temporary helper binary.
pub(crate) fn run_updater_helper(args: ApplyUpdateArgs) {
    let helper_exe = std::env::current_exe().ok();
    let result = apply_update(&args);

    // Ensure the parent has completely exited before relaunching so persistence
    // files are flushed and no two RimMod windows overlap.
    wait_for_parent_exit(args.parent_pid);

    let error_message = result.err();
    let _ = spawn_relaunched_app(
        &args.target_exe,
        helper_exe.as_deref(),
        error_message.as_deref(),
    );
}

fn apply_update(args: &ApplyUpdateArgs) -> Result<(), String> {
    if !args
        .download_url
        .starts_with(GITHUB_RELEASE_DOWNLOAD_PREFIX)
    {
        return Err("refusing to download an update from an untrusted URL".to_owned());
    }

    let staging_dir = std::env::temp_dir().join(format!("rimmod-update-{}", std::process::id()));
    let _ = fs::remove_dir_all(&staging_dir);
    fs::create_dir_all(&staging_dir)
        .map_err(|error| format!("could not create the temporary update folder: {error}"))?;

    let outcome = (|| {
        let archive_path = staging_dir.join(&args.asset_name);
        download_archive(&args.download_url, &archive_path)?;

        let staged_binary =
            extract_binary_from_archive(&archive_path, &args.asset_name, &staging_dir)?;

        wait_for_parent_exit(args.parent_pid);
        replace_binary(&args.target_exe, &staged_binary)
    })();

    let _ = fs::remove_dir_all(&staging_dir);
    outcome
}

fn download_archive(url: &str, destination: &Path) -> Result<(), String> {
    let client = http_client(DOWNLOAD_TIMEOUT)?;
    let mut response = client
        .get(url)
        .send()
        .map_err(|error| format!("could not download the update: {error}"))?;

    if !response.status().is_success() {
        return Err(format!(
            "update download failed with status {}",
            response.status()
        ));
    }

    let mut file = File::create(destination)
        .map_err(|error| format!("could not create the temporary archive: {error}"))?;

    let bytes = io::copy(&mut response, &mut file)
        .map_err(|error| format!("could not write the downloaded update: {error}"))?;

    file.flush()
        .map_err(|error| format!("could not flush the downloaded update: {error}"))?;

    if bytes == 0 {
        return Err("the downloaded update archive was empty".to_owned());
    }

    Ok(())
}

fn expected_binary_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "rimmod.exe"
    } else {
        "rimmod"
    }
}

fn extract_binary_from_archive(
    archive_path: &Path,
    asset_name: &str,
    staging_dir: &Path,
) -> Result<PathBuf, String> {
    let output_path = staging_dir.join(format!("extracted-{}", expected_binary_name()));

    if asset_name.ends_with(".zip") {
        extract_binary_from_zip(archive_path, expected_binary_name(), &output_path)?;
        return Ok(output_path);
    }

    if asset_name.ends_with(".tar.gz") {
        extract_binary_from_tar_gz(
            archive_path,
            expected_binary_name(),
            staging_dir,
            &output_path,
        )?;
        return Ok(output_path);
    }

    Err(format!("unsupported update archive format: {asset_name}"))
}

pub(crate) fn extract_binary_from_zip(
    archive_path: &Path,
    expected_file_name: &str,
    output_path: &Path,
) -> Result<(), String> {
    let file = File::open(archive_path)
        .map_err(|error| format!("could not open the downloaded zip archive: {error}"))?;
    let mut archive = ZipArchive::new(file)
        .map_err(|error| format!("the downloaded zip archive is invalid: {error}"))?;

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| format!("could not read the zip archive: {error}"))?;

        if entry.is_dir() {
            continue;
        }

        let Some(entry_name) = entry.enclosed_name().and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        }) else {
            continue;
        };

        if !entry_name.eq_ignore_ascii_case(expected_file_name) {
            continue;
        }

        let mut output = File::create(output_path)
            .map_err(|error| format!("could not create the extracted executable: {error}"))?;
        let bytes = io::copy(&mut entry, &mut output)
            .map_err(|error| format!("could not extract the updated executable: {error}"))?;
        output
            .flush()
            .map_err(|error| format!("could not flush the extracted executable: {error}"))?;

        if bytes == 0 {
            return Err("the extracted executable was empty".to_owned());
        }

        set_executable_permissions(output_path)?;
        return Ok(());
    }

    Err(format!(
        "the update archive did not contain {expected_file_name}"
    ))
}

fn extract_binary_from_tar_gz(
    archive_path: &Path,
    expected_file_name: &str,
    staging_dir: &Path,
    output_path: &Path,
) -> Result<(), String> {
    let extract_dir = staging_dir.join("extracted");
    fs::create_dir_all(&extract_dir)
        .map_err(|error| format!("could not create the extraction folder: {error}"))?;

    let output = Command::new("tar")
        .arg("-xzf")
        .arg(archive_path)
        .arg("-C")
        .arg(&extract_dir)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("could not run tar to extract the update: {error}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "could not extract the tar.gz archive: {}",
            stderr.trim()
        ));
    }

    let extracted = find_file_named(&extract_dir, expected_file_name)
        .ok_or_else(|| format!("the update archive did not contain {expected_file_name}"))?;

    let metadata = fs::metadata(&extracted)
        .map_err(|error| format!("could not inspect the extracted executable: {error}"))?;
    if metadata.len() == 0 {
        return Err("the extracted executable was empty".to_owned());
    }

    fs::copy(&extracted, output_path)
        .map_err(|error| format!("could not copy the extracted executable: {error}"))?;
    set_executable_permissions(output_path)?;

    Ok(())
}

fn find_file_named(root: &Path, expected_file_name: &str) -> Option<PathBuf> {
    let entries = fs::read_dir(root).ok()?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file()
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.eq_ignore_ascii_case(expected_file_name))
        {
            return Some(path);
        }

        if path.is_dir()
            && let Some(found) = find_file_named(&path, expected_file_name)
        {
            return Some(found);
        }
    }

    None
}

pub(crate) fn replace_binary(target_exe: &Path, staged_binary: &Path) -> Result<(), String> {
    let target_dir = target_exe
        .parent()
        .ok_or_else(|| "could not determine the destination folder".to_owned())?;

    // Stage into the destination directory first so renames stay on one filesystem.
    let next_path = target_dir.join(format!(".rimmod-next-{}", std::process::id()));
    let _ = fs::remove_file(&next_path);

    fs::copy(staged_binary, &next_path)
        .map_err(|error| format!("could not copy the update into place: {error}"))?;
    set_executable_permissions(&next_path)?;

    let swap_result = swap_executable(&next_path, target_exe);
    if swap_result.is_err() {
        let _ = fs::remove_file(&next_path);
    }

    swap_result
}

#[cfg(target_os = "windows")]
fn swap_executable(next_path: &Path, target_exe: &Path) -> Result<(), String> {
    let backup_path = backup_path_for(target_exe);
    let _ = fs::remove_file(&backup_path);

    // Retry briefly in case Windows or antivirus still holds a handle on the
    // exiting executable right after the process terminates.
    let mut last_error = None;
    for attempt in 0..REPLACE_RETRY_ATTEMPTS {
        match fs::rename(target_exe, &backup_path) {
            Ok(()) => {
                last_error = None;
                break;
            }
            Err(error) => {
                last_error = Some(error);
                if attempt + 1 < REPLACE_RETRY_ATTEMPTS {
                    std::thread::sleep(REPLACE_RETRY_DELAY);
                }
            }
        }
    }

    if let Some(error) = last_error {
        return Err(format!(
            "could not move the current executable aside: {error}"
        ));
    }

    if let Err(error) = fs::rename(next_path, target_exe) {
        let _ = fs::rename(&backup_path, target_exe);
        return Err(format!(
            "could not move the new executable into place: {error}"
        ));
    }

    let _ = fs::remove_file(&backup_path);
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn swap_executable(next_path: &Path, target_exe: &Path) -> Result<(), String> {
    let mut last_error = None;
    for attempt in 0..REPLACE_RETRY_ATTEMPTS {
        match fs::rename(next_path, target_exe) {
            Ok(()) => return Ok(()),
            Err(error) => {
                last_error = Some(error);
                if attempt + 1 < REPLACE_RETRY_ATTEMPTS {
                    std::thread::sleep(REPLACE_RETRY_DELAY);
                }
            }
        }
    }

    Err(format!(
        "could not move the new executable into place: {}",
        last_error
            .map(|error| error.to_string())
            .unwrap_or_else(|| "unknown error".to_owned())
    ))
}

fn spawn_relaunched_app(
    target_exe: &Path,
    helper_exe: Option<&Path>,
    update_error: Option<&str>,
) -> io::Result<()> {
    let mut command = Command::new(target_exe);

    if let Some(parent_dir) = target_exe.parent() {
        command.current_dir(parent_dir);
    }

    if let Some(helper) = helper_exe {
        command.arg(ARG_CLEANUP_UPDATER).arg(helper);
    }

    if let Some(error) = update_error {
        command.arg(ARG_UPDATE_FAILED).arg(error);
    }

    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    configure_detached(&mut command);

    command.spawn().map(|_child| ())
}

/// Remove the temporary updater helper and any leftover `.old` backup binary in
/// the background so normal startup is never delayed.
pub(crate) fn cleanup_after_update(cleanup_updater: Option<PathBuf>) {
    let backup_path = std::env::current_exe()
        .ok()
        .map(|exe| backup_path_for(&exe));
    let cleanup_path = cleanup_updater.filter(|path| is_safe_cleanup_path(path));

    if cleanup_path.is_none() && backup_path.as_ref().is_none_or(|path| !path.exists()) {
        return;
    }

    std::thread::spawn(move || {
        if let Some(path) = cleanup_path {
            remove_file_with_retry(&path);
        }

        if let Some(backup) = backup_path
            && backup.exists()
        {
            remove_file_with_retry(&backup);
        }
    });
}

pub(crate) fn is_safe_cleanup_path(path: &Path) -> bool {
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };

    if !file_name.starts_with(UPDATER_FILE_PREFIX) {
        return false;
    }

    let Some(parent) = path.parent() else {
        return false;
    };

    let temp_dir = std::env::temp_dir();
    if parent == temp_dir {
        return true;
    }

    match (fs::canonicalize(parent), fs::canonicalize(temp_dir)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

fn remove_file_with_retry(path: &Path) {
    for attempt in 0..CLEANUP_RETRY_ATTEMPTS {
        match fs::remove_file(path) {
            Ok(()) => return,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return,
            Err(_) if attempt + 1 < CLEANUP_RETRY_ATTEMPTS => {
                std::thread::sleep(CLEANUP_RETRY_DELAY);
            }
            Err(_) => return,
        }
    }
}

#[cfg(target_os = "windows")]
fn configure_detached(command: &mut Command) {
    use std::os::windows::process::CommandExt;

    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    command.creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP);
}

#[cfg(not(target_os = "windows"))]
fn configure_detached(_command: &mut Command) {}

#[cfg(unix)]
fn set_executable_permissions(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path)
        .map_err(|error| format!("could not read permissions on {}: {error}", path.display()))?
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)
        .map_err(|error| format!("could not mark {} executable: {error}", path.display()))
}

#[cfg(not(unix))]
fn set_executable_permissions(_path: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(target_os = "windows")]
fn wait_for_parent_exit(parent_pid: u32) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
    };

    // SAFETY: OpenProcess returns either a valid handle or null (when the
    // process already exited or cannot be opened). Any non-null handle is
    // waited on and immediately closed.
    unsafe {
        let handle = OpenProcess(PROCESS_SYNCHRONIZE, 0, parent_pid);
        if handle.is_null() {
            return;
        }

        let timeout_ms = PARENT_EXIT_TIMEOUT.as_millis().min(u32::MAX as u128) as u32;
        let _ = WaitForSingleObject(handle, timeout_ms);
        let _ = CloseHandle(handle);
    }
}

#[cfg(target_os = "linux")]
fn wait_for_parent_exit(parent_pid: u32) {
    use std::io::Read;
    use std::time::Instant;

    let deadline = Instant::now() + PARENT_EXIT_TIMEOUT;
    let stat_path = PathBuf::from(format!("/proc/{parent_pid}/stat"));

    while Instant::now() < deadline {
        let mut contents = String::new();
        let Ok(mut file) = File::open(&stat_path) else {
            return;
        };

        if file.read_to_string(&mut contents).is_err() {
            return;
        }

        // `/proc/<pid>/stat` places the single-character process state right
        // after the closing `)` of the command name.
        let is_zombie = contents
            .rsplit_once(')')
            .and_then(|(_, rest)| rest.split_whitespace().next())
            .is_some_and(|state| state == "Z" || state == "X");

        if is_zombie {
            return;
        }

        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn wait_for_parent_exit(_parent_pid: u32) {
    std::thread::sleep(Duration::from_millis(500));
}

#[cfg(test)]
#[path = "../../tests/unit/updater.rs"]
mod tests;
