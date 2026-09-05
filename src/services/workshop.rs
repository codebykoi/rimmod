use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde::Deserialize;

use crate::models::major_minor_version;

pub(crate) const RIMWORLD_APP_ID: u32 = 294_100;
const QUERY_FILES_URL: &str = "https://api.steampowered.com/IPublishedFileService/QueryFiles/v1/";

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkshopSort {
    #[default]
    Popular,
    MostSubscribed,
    Recent,
    Updated,
}

impl WorkshopSort {
    pub(crate) const ALL: [Self; 4] = [
        Self::Popular,
        Self::MostSubscribed,
        Self::Recent,
        Self::Updated,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Popular => "Popular",
            Self::MostSubscribed => "Most subscribed",
            Self::Recent => "Newest",
            Self::Updated => "Recently updated",
        }
    }

    fn query_type(self, has_search_text: bool) -> u32 {
        if has_search_text {
            // Steam provides a dedicated relevance ranking for text searches.
            return 12;
        }

        match self {
            Self::Popular => 0,
            Self::MostSubscribed => 9,
            Self::Recent => 1,
            Self::Updated => 21,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkshopItem {
    pub(crate) published_file_id: u64,
    pub(crate) title: String,
    pub(crate) description: String,
    pub(crate) preview_url: Option<String>,
    pub(crate) subscriptions: Option<u64>,
    /// Versions the author tagged the item with on the Workshop
    pub(crate) supported_versions: Vec<String>,
}

impl WorkshopItem {
    pub(crate) fn page_url(&self) -> String {
        format!(
            "https://steamcommunity.com/sharedfiles/filedetails/?id={}",
            self.published_file_id
        )
    }
}

#[derive(Debug)]
pub(crate) struct WorkshopPage {
    pub(crate) items: Vec<WorkshopItem>,
    pub(crate) total: u64,
}

#[derive(Deserialize)]
struct QueryFilesEnvelope {
    response: QueryFilesResponse,
}

#[derive(Deserialize)]
struct QueryFilesResponse {
    total: u64,
    #[serde(default)]
    publishedfiledetails: Vec<PublishedFileDetails>,
}

#[derive(Deserialize)]
struct PublishedFileDetails {
    publishedfileid: String,
    #[serde(default)]
    title: String,
    #[serde(default, alias = "file_description")]
    short_description: String,
    #[serde(default)]
    preview_url: String,
    #[serde(default, deserialize_with = "deserialize_optional_string_number")]
    subscriptions: Option<u64>,
    #[serde(default)]
    tags: Vec<SteamTag>,
}

#[derive(Deserialize)]
struct SteamTag {
    #[serde(default)]
    tag: String,
}

/// RimWorld's uploader tags items with their supported versions as plain
/// Steam tags such as "1.6" next to unrelated tags like "Mod".
fn extract_supported_versions(tags: Vec<SteamTag>) -> Vec<String> {
    let mut versions: Vec<String> = tags
        .into_iter()
        .map(|tag| tag.tag)
        .filter(|tag| major_minor_version(tag).is_some())
        .collect();

    versions.sort_by_key(|version| major_minor_version(version));
    versions.dedup();
    versions
}

fn deserialize_optional_string_number<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrNumber {
        String(String),
        Number(u64),
    }

    match Option::<StringOrNumber>::deserialize(deserializer)? {
        Some(StringOrNumber::String(value)) => {
            value.parse().map(Some).map_err(serde::de::Error::custom)
        }
        Some(StringOrNumber::Number(value)) => Ok(Some(value)),
        None => Ok(None),
    }
}

pub(crate) fn query_workshop(
    api_key: &str,
    search_text: &str,
    sort: WorkshopSort,
    page: u32,
) -> Result<WorkshopPage, String> {
    let api_key = api_key.trim();
    if api_key.is_empty() {
        return Err("Add a Steam Web API key in Settings before searching".to_owned());
    }

    let search_text = search_text.trim();
    let input = serde_json::json!({
        "query_type": sort.query_type(!search_text.is_empty()),
        "page": page.max(1),
        "numperpage": 30,
        "creator_appid": RIMWORLD_APP_ID,
        "appid": RIMWORLD_APP_ID,
        "requiredtags": [],
        "excludedtags": [],
        "match_all_tags": false,
        "required_flags": [],
        "omitted_flags": [],
        "search_text": search_text,
        "filetype": 0,
        "child_publishedfileid": "0",
        "days": 7,
        "include_recent_votes_only": false,
        "cache_max_age_seconds": 300,
        "language": 0,
        "totalonly": false,
        "ids_only": false,
        "return_vote_data": true,
        "return_tags": true,
        "return_kv_tags": false,
        "return_previews": false,
        "return_children": false,
        "return_short_description": true,
        "return_for_sale_data": false,
        "return_metadata": false,
        "return_playtime_stats": 0
    });

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(concat!("RimMod/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|error| format!("could not create the Steam API client: {error}"))?;

    let response = client
        .get(QUERY_FILES_URL)
        .query(&[("key", api_key), ("input_json", &input.to_string())])
        .send()
        .map_err(|error| format!("Steam API request failed: {error}"))?
        .error_for_status()
        .map_err(|error| format!("Steam API rejected the request: {error}"))?;

    let envelope = response
        .json::<QueryFilesEnvelope>()
        .map_err(|error| format!("Steam API returned an unexpected response: {error}"))?;

    let items = envelope
        .response
        .publishedfiledetails
        .into_iter()
        .filter_map(|details| {
            let published_file_id = details.publishedfileid.parse().ok()?;

            Some(WorkshopItem {
                published_file_id,
                title: details.title,
                description: details.short_description,
                preview_url: (!details.preview_url.is_empty()).then_some(details.preview_url),
                subscriptions: details.subscriptions,
                supported_versions: extract_supported_versions(details.tags),
            })
        })
        .collect();

    Ok(WorkshopPage {
        items,
        total: envelope.response.total,
    })
}

#[derive(Debug)]
pub(crate) struct InstallOutcome {
    pub(crate) published_file_id: u64,
    pub(crate) error: Option<String>,
}

pub(crate) fn install_workshop_items(
    steamcmd_path: &Path,
    published_file_ids: &[u64],
) -> io::Result<Vec<InstallOutcome>> {
    let mut unique_ids = Vec::with_capacity(published_file_ids.len());
    for id in published_file_ids {
        if !unique_ids.contains(id) {
            unique_ids.push(*id);
        }
    }

    let mut args = vec!["+login".to_owned(), "anonymous".to_owned()];
    for id in &unique_ids {
        args.push("+workshop_download_item".to_owned());
        args.push(RIMWORLD_APP_ID.to_string());
        args.push(id.to_string());
    }
    args.push("+quit".to_owned());

    let output = Command::new(steamcmd_path).args(&args).output()?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{stdout}\n{stderr}");
    let lowercase = combined.to_ascii_lowercase();
    let global_failure =
        !output.status.success() || steamcmd_output_reports_failure(&stdout, &stderr);

    let outcomes = unique_ids
        .iter()
        .map(|&id| {
            if contains_item_marker(&lowercase, "success. downloaded item ", id) {
                return InstallOutcome {
                    published_file_id: id,
                    error: None,
                };
            }

            if let Some(line) = combined.lines().find(|line| {
                contains_item_marker(&line.to_ascii_lowercase(), "error! download item ", id)
            }) {
                return InstallOutcome {
                    published_file_id: id,
                    error: Some(line.trim().to_owned()),
                };
            }

            if global_failure {
                return InstallOutcome {
                    published_file_id: id,
                    error: Some(steamcmd_exit_message(&stderr, &stdout, output.status)),
                };
            }

            InstallOutcome {
                published_file_id: id,
                error: None,
            }
        })
        .collect();

    Ok(outcomes)
}

fn contains_item_marker(lowercase: &str, prefix: &str, id: u64) -> bool {
    let marker = format!("{prefix}{id}");
    let bytes = lowercase.as_bytes();
    let mut start = 0;

    while let Some(offset) = lowercase[start..].find(&marker) {
        let after_marker = start + offset + marker.len();
        match bytes.get(after_marker) {
            Some(next) if next.is_ascii_digit() => start += 1,
            _ => return true,
        }
    }

    false
}

fn steamcmd_exit_message(stderr: &str, stdout: &str, status: std::process::ExitStatus) -> String {
    let details = stderr
        .lines()
        .chain(stdout.lines())
        .find(|line| {
            let lowercase = line.to_ascii_lowercase();
            lowercase.contains("error") || lowercase.contains("failed")
        })
        .or_else(|| stderr.lines().chain(stdout.lines()).next_back())
        .unwrap_or_default()
        .trim();

    if details.is_empty() {
        format!("SteamCMD exited with {status}")
    } else if status.success() {
        format!("SteamCMD reported a failure: {details}")
    } else {
        format!("SteamCMD exited with {status}: {details}")
    }
}

fn steamcmd_output_reports_failure(stdout: &str, stderr: &str) -> bool {
    let output = format!("{stdout}\n{stderr}").to_ascii_lowercase();
    output.contains("error! download item")
        || output.contains("download item") && output.contains("failed (")
}

pub(crate) fn is_item_installed(
    workshop_path: Option<&Path>,
    steamcmd_workshop_path: Option<&Path>,
    published_file_id: u64,
) -> bool {
    [workshop_path, steamcmd_workshop_path]
        .into_iter()
        .flatten()
        .any(|path| path.join(published_file_id.to_string()).is_dir())
}

pub(crate) fn default_steamcmd_path() -> PathBuf {
    PathBuf::from(if cfg!(target_os = "windows") {
        "steamcmd.exe"
    } else {
        "steamcmd"
    })
}

pub(crate) fn steamcmd_workshop_path(configured_steamcmd_path: Option<&Path>) -> Option<PathBuf> {
    let steamcmd_path = configured_steamcmd_path
        .map(Path::to_path_buf)
        .unwrap_or_else(default_steamcmd_path);
    let resolved_steamcmd_path = resolve_executable(&steamcmd_path)?;
    let steamcmd_folder = resolved_steamcmd_path.parent()?;

    Some(
        steamcmd_folder
            .join("steamapps")
            .join("workshop")
            .join("content")
            .join(RIMWORLD_APP_ID.to_string()),
    )
}

fn resolve_executable(executable: &Path) -> Option<PathBuf> {
    if executable.is_file() {
        return Some(
            executable
                .canonicalize()
                .unwrap_or_else(|_| executable.to_path_buf()),
        );
    }

    let is_bare_name = executable
        .parent()
        .is_none_or(|parent| parent.as_os_str().is_empty());
    if !is_bare_name {
        return None;
    }

    let path_variable = std::env::var_os("PATH")?;
    std::env::split_paths(&path_variable).find_map(|folder| {
        let candidate = folder.join(executable);
        candidate
            .is_file()
            .then(|| candidate.canonicalize().unwrap_or(candidate))
    })
}

#[cfg(test)]
#[path = "../../tests/unit/workshop.rs"]
mod tests;
