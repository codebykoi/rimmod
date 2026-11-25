use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde::Deserialize;

pub(crate) mod bbcode;

use crate::models::major_minor_version;

pub(crate) const RIMWORLD_APP_ID: u32 = 294_100;
const QUERY_FILES_URL: &str = "https://api.steampowered.com/IPublishedFileService/QueryFiles/v1/";
const GET_USER_FILES_URL: &str =
    "https://api.steampowered.com/IPublishedFileService/GetUserFiles/v1/";
const GET_DETAILS_URL: &str = "https://api.steampowered.com/IPublishedFileService/GetDetails/v1/";
const PUBLISHED_FILE_DETAILS_URL: &str =
    "https://api.steampowered.com/ISteamRemoteStorage/GetPublishedFileDetails/v1/";
const SUBSCRIBE_URL: &str = "https://api.steampowered.com/IPublishedFileService/Subscribe/v1/";
const UNSUBSCRIBE_URL: &str = "https://api.steampowered.com/IPublishedFileService/Unsubscribe/v1/";
const PLAYER_SUMMARIES_URL: &str = "https://api.steampowered.com/ISteamUser/GetPlayerSummaries/v2/";
const ACF_FILE_NAME: &str = "appworkshop_294100.acf";

/// RimWorld Workshop version tags, newest first. Steam returns the same tags
/// on items, so they are safe to use as query filters.
pub(crate) const RIMWORLD_VERSIONS: &[&str] =
    &["1.7", "1.6", "1.5", "1.4", "1.3", "1.2", "1.1", "1.0"];

/// RimWorld Workshop DLC tags.
pub(crate) const RIMWORLD_DLC: &[&str] = &["Royalty", "Ideology", "Biotech", "Anomaly", "Odyssey"];

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkshopSort {
    /// Steam's dedicated relevance ranking for text searches.
    Relevance,
    #[default]
    Trending,
    TopRated,
    MostSubscribed,
    Recent,
    Updated,
}

impl WorkshopSort {
    pub(crate) const ALL: [Self; 5] = [
        Self::Trending,
        Self::TopRated,
        Self::MostSubscribed,
        Self::Recent,
        Self::Updated,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Relevance => "Search relevance",
            Self::Trending => "Trending",
            Self::TopRated => "Top rated",
            Self::MostSubscribed => "Most subscribed",
            Self::Recent => "Newest",
            Self::Updated => "Recently updated",
        }
    }

    fn query_type(self) -> u32 {
        match self {
            Self::Relevance => 12,
            Self::Trending => 3,
            Self::TopRated => 11,
            Self::MostSubscribed => 9,
            Self::Recent => 1,
            Self::Updated => 21,
        }
    }
}

/// Ordering used by the local "Subscribed" list, where Steam query rankings
/// are not available because the items are already filtered to the account.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum SubscribedSort {
    #[default]
    RecentlyUpdated,
    MostSubscribed,
    Name,
}

impl SubscribedSort {
    pub(crate) const ALL: [Self; 3] = [Self::RecentlyUpdated, Self::MostSubscribed, Self::Name];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::RecentlyUpdated => "Recently updated",
            Self::MostSubscribed => "Most subscribed",
            Self::Name => "Name",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct WorkshopQuery {
    pub(crate) search_text: String,
    pub(crate) sort: WorkshopSort,
    pub(crate) page: u32,
    pub(crate) versions: Vec<String>,
    pub(crate) dlc: Vec<String>,
}

impl WorkshopQuery {
    fn tags(&self) -> Vec<String> {
        let mut tags = self.versions.clone();
        tags.extend(self.dlc.iter().cloned());
        tags
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkshopItem {
    pub(crate) published_file_id: u64,
    pub(crate) title: String,
    pub(crate) description: String,
    pub(crate) preview_url: Option<String>,
    /// Additional preview image URLs returned by Steam for this item.
    pub(crate) images: Vec<String>,
    pub(crate) author_steam_id: Option<u64>,
    pub(crate) author: Option<String>,
    pub(crate) subscriptions: Option<u64>,
    pub(crate) favorites: Option<u64>,
    pub(crate) time_created: u64,
    pub(crate) time_updated: u64,
    pub(crate) votes_up: u64,
    pub(crate) votes_down: u64,
    /// All Workshop tags, including the version and DLC tags.
    pub(crate) tags: Vec<String>,
    /// Versions the author tagged the item with on the Workshop.
    pub(crate) supported_versions: Vec<String>,
}

impl WorkshopItem {
    pub(crate) fn page_url(&self) -> String {
        format!(
            "https://steamcommunity.com/sharedfiles/filedetails/?id={}",
            self.published_file_id
        )
    }

    /// Opens the item page inside the Steam client, where the account's own
    /// Subscribe button is always available.
    pub(crate) fn steam_url(&self) -> String {
        format!("steam://url/CommunityFilePage/{}", self.published_file_id)
    }

    pub(crate) fn author_url(&self) -> Option<String> {
        self.author_steam_id
            .map(|steam_id| format!("https://steamcommunity.com/profiles/{steam_id}"))
    }

    /// Percentage of positive votes, or `None` when the item has no votes.
    pub(crate) fn rating_percent(&self) -> Option<u32> {
        let total = self.votes_up.saturating_add(self.votes_down);
        if total == 0 {
            return None;
        }

        Some((self.votes_up.saturating_mul(100) / total) as u32)
    }

    pub(crate) fn supports_version(&self, game_version: &str) -> bool {
        let Some(game_version) = major_minor_version(game_version) else {
            return false;
        };

        self.supported_versions
            .iter()
            .any(|version| major_minor_version(version) == Some(game_version))
    }

    /// Preview images in slideshow order: the cover thumbnail first, followed
    /// by Steam's additional preview screenshots without duplicates.
    pub(crate) fn preview_slideshow_urls(&self) -> Vec<&str> {
        let mut urls =
            Vec::with_capacity(self.images.len() + usize::from(self.preview_url.is_some()));

        if let Some(preview_url) = self.preview_url.as_deref() {
            urls.push(preview_url);
        }

        for image in &self.images {
            let url = image.as_str();
            if !urls.contains(&url) {
                urls.push(url);
            }
        }

        urls
    }
}

#[derive(Debug)]
pub(crate) struct WorkshopPage {
    pub(crate) items: Vec<WorkshopItem>,
    pub(crate) total: u64,
}

/// The account's subscriptions, used for the Subscribed tab and to label
/// browse results.
#[derive(Debug, Default)]
pub(crate) struct SubscriptionSnapshot {
    pub(crate) ids: HashSet<u64>,
    pub(crate) items: Vec<WorkshopItem>,
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
struct FileDetailsEnvelope {
    response: FileDetailsResponse,
}

#[derive(Deserialize)]
struct FileDetailsResponse {
    #[serde(default)]
    publishedfiledetails: Vec<PublishedFileDetails>,
}

#[derive(Deserialize)]
struct UserFilesEnvelope {
    response: UserFilesResponse,
}

#[derive(Deserialize)]
struct UserFilesResponse {
    #[serde(default)]
    total: u64,
    #[serde(default)]
    publishedfiledetails: Vec<PublishedFileDetails>,
}

#[derive(Deserialize)]
struct PublishedFileDetails {
    publishedfileid: String,
    #[serde(default)]
    result: u32,
    #[serde(default)]
    creator: String,
    #[serde(default)]
    title: String,
    #[serde(
        default,
        rename = "description",
        alias = "file_description",
        alias = "short_description"
    )]
    description: String,
    #[serde(default)]
    preview_url: String,
    #[serde(default, deserialize_with = "deserialize_optional_string_number")]
    subscriptions: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_optional_string_number")]
    favorited: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_optional_string_number")]
    time_created: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_optional_string_number")]
    time_updated: Option<u64>,
    #[serde(default)]
    vote_data: Option<VoteData>,
    #[serde(default)]
    tags: Vec<SteamTag>,
    #[serde(default)]
    previews: Vec<SteamPreview>,
}

impl PublishedFileDetails {
    fn into_item(self) -> Option<WorkshopItem> {
        // Steam reports per-file results; 1 is "ok", 9 is "not found".
        if self.result > 1 {
            return None;
        }

        let published_file_id = self.publishedfileid.parse().ok()?;
        let tags: Vec<String> = self
            .tags
            .into_iter()
            .map(|tag| tag.tag.trim().to_owned())
            .filter(|tag| !tag.is_empty())
            .collect();
        let supported_versions = extract_supported_versions(&tags);
        let vote_data = self.vote_data.unwrap_or_default();
        let images = extract_preview_image_urls(&self.previews);

        Some(WorkshopItem {
            published_file_id,
            title: self.title,
            description: self.description,
            preview_url: (!self.preview_url.is_empty()).then_some(self.preview_url),
            images,
            author_steam_id: self.creator.parse().ok().filter(|id| *id != 0),
            author: None,
            subscriptions: self.subscriptions,
            favorites: self.favorited,
            time_created: self.time_created.unwrap_or(0),
            time_updated: self.time_updated.unwrap_or(0),
            votes_up: vote_data.votes_up(),
            votes_down: vote_data.votes_down(),
            tags,
            supported_versions,
        })
    }
}

#[derive(Deserialize, Default)]
struct VoteData {
    #[serde(default, deserialize_with = "deserialize_optional_string_number")]
    votes_up: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_optional_string_number")]
    votes_down: Option<u64>,
}

impl VoteData {
    fn votes_up(&self) -> u64 {
        self.votes_up.unwrap_or(0)
    }

    fn votes_down(&self) -> u64 {
        self.votes_down.unwrap_or(0)
    }
}

#[derive(Deserialize)]
struct SteamTag {
    #[serde(default)]
    tag: String,
}

#[derive(Deserialize)]
struct SteamPreview {
    #[serde(default)]
    url: String,
    #[serde(default, deserialize_with = "deserialize_optional_string_number")]
    sortorder: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_optional_string_number")]
    preview_type: Option<u64>,
}

/// RimWorld's uploader tags items with their supported versions as plain
/// Steam tags such as "1.6" next to unrelated tags like "Mod".
fn extract_supported_versions(tags: &[String]) -> Vec<String> {
    let mut versions: Vec<String> = tags
        .iter()
        .filter(|tag| major_minor_version(tag).is_some())
        .cloned()
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

fn http_client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(concat!("RimMod/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|error| format!("could not create the Steam API client: {error}"))
}

fn steam_status_error(action: &str, status: reqwest::StatusCode) -> String {
    match status.as_u16() {
        401 | 403 => format!(
            "Steam rejected the {action} request. The Web API key must belong to the \
             Steam account that owns RimWorld."
        ),
        429 => "Steam is rate limiting requests. Wait a moment and try again.".to_owned(),
        _ => format!("Steam rejected the {action} request ({status})"),
    }
}

fn query_input(query: &WorkshopQuery) -> serde_json::Value {
    let search_text = query.search_text.trim();
    let tags = query.tags();

    serde_json::json!({
        "query_type": query.sort.query_type(),
        "page": query.page.max(1),
        "numperpage": 30,
        "creator_appid": RIMWORLD_APP_ID,
        "appid": RIMWORLD_APP_ID,
        "requiredtags": tags,
        "excludedtags": [],
        "match_all_tags": true,
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
        "return_previews": true,
        "return_children": false,
        "return_short_description": false,
        "return_for_sale_data": false,
        "return_metadata": false,
        "return_playtime_stats": 0
    })
}

pub(crate) fn query_workshop(api_key: &str, query: &WorkshopQuery) -> Result<WorkshopPage, String> {
    let api_key = api_key.trim();
    if api_key.is_empty() {
        return Err("Add a Steam Web API key in Settings before searching".to_owned());
    }

    let client = http_client()?;
    let input = query_input(query);
    let response = client
        .get(QUERY_FILES_URL)
        .query(&[("key", api_key), ("input_json", &input.to_string())])
        .send()
        .map_err(|error| format!("Steam API request failed: {error}"))?;

    let status = response.status();
    if !status.is_success() {
        return Err(steam_status_error("catalog search", status));
    }

    let envelope = response
        .json::<QueryFilesEnvelope>()
        .map_err(|error| format!("Steam API returned an unexpected response: {error}"))?;

    let mut items: Vec<WorkshopItem> = envelope
        .response
        .publishedfiledetails
        .into_iter()
        .filter_map(PublishedFileDetails::into_item)
        .collect();
    attach_author_names(api_key, &mut items);

    Ok(WorkshopPage {
        items,
        total: envelope.response.total,
    })
}

fn details_query_params(api_key: &str, ids: &[u64]) -> Vec<(String, String)> {
    let mut params = Vec::with_capacity(ids.len() + 4);
    params.push(("key".to_owned(), api_key.to_owned()));
    params.push(("includetags".to_owned(), "true".to_owned()));
    params.push(("includeadditionalpreviews".to_owned(), "true".to_owned()));
    params.push(("includevotes".to_owned(), "true".to_owned()));

    for (index, id) in ids.iter().enumerate() {
        params.push((format!("publishedfileids[{index}]"), id.to_string()));
    }

    params
}

fn fetch_items_by_ids_with_key(
    client: &reqwest::blocking::Client,
    api_key: &str,
    ids: &[u64],
) -> Result<Vec<WorkshopItem>, String> {
    let mut items = Vec::with_capacity(ids.len());

    for chunk in ids.chunks(50) {
        let params = details_query_params(api_key, chunk);
        let response = client
            .get(GET_DETAILS_URL)
            .query(&params)
            .send()
            .map_err(|error| format!("Steam API request failed: {error}"))?;

        let status = response.status();
        if !status.is_success() {
            return Err(steam_status_error("item details", status));
        }

        let envelope = response
            .json::<FileDetailsEnvelope>()
            .map_err(|error| format!("Steam API returned an unexpected response: {error}"))?;

        items.extend(
            envelope
                .response
                .publishedfiledetails
                .into_iter()
                .filter_map(PublishedFileDetails::into_item),
        );
    }

    Ok(items)
}

/// Fetch full item details by ID. Uses `IPublishedFileService/GetDetails` when
/// an API key is available (so additional previews and vote data are included),
/// and falls back to the keyless `GetPublishedFileDetails` endpoint otherwise.
pub(crate) fn fetch_items_by_ids(
    ids: &[u64],
    api_key: Option<&str>,
) -> Result<Vec<WorkshopItem>, String> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }

    let mut unique_ids = Vec::with_capacity(ids.len());
    for &id in ids {
        if !unique_ids.contains(&id) {
            unique_ids.push(id);
        }
    }

    let client = http_client()?;
    let trimmed_key = api_key.map(str::trim).filter(|key| !key.is_empty());

    let mut items = if let Some(api_key) = trimmed_key
        && let Ok(items) = fetch_items_by_ids_with_key(&client, api_key, &unique_ids)
    {
        items
    } else {
        let mut items = Vec::with_capacity(unique_ids.len());

        for chunk in unique_ids.chunks(100) {
            let mut form: Vec<(String, String)> = Vec::with_capacity(chunk.len() + 1);
            form.push(("itemcount".to_owned(), chunk.len().to_string()));

            for (index, id) in chunk.iter().enumerate() {
                form.push((format!("publishedfileids[{index}]"), id.to_string()));
            }

            let response = client
                .post(PUBLISHED_FILE_DETAILS_URL)
                .form(&form)
                .send()
                .map_err(|error| format!("Steam API request failed: {error}"))?;

            let status = response.status();
            if !status.is_success() {
                return Err(steam_status_error("item details", status));
            }

            let envelope = response
                .json::<FileDetailsEnvelope>()
                .map_err(|error| format!("Steam API returned an unexpected response: {error}"))?;

            items.extend(
                envelope
                    .response
                    .publishedfiledetails
                    .into_iter()
                    .filter_map(PublishedFileDetails::into_item),
            );
        }

        items
    };

    if let Some(api_key) = trimmed_key {
        attach_author_names(api_key, &mut items);
    }

    Ok(items)
}

fn user_files_input(steamid: u64, page: u32) -> serde_json::Value {
    serde_json::json!({
        "steamid": steamid.to_string(),
        "appid": RIMWORLD_APP_ID,
        "page": page,
        "numperpage": 100,
        "type": "mysubscriptions",
        "return_vote_data": true,
        "return_tags": true,
        "return_previews": true,
        "return_short_description": false,
        "cache_max_age_seconds": 300
    })
}

/// Fetch every item the signed-in account is subscribed to. The Web API key
/// identifies the account, and `steamid` selects whose list to read.
pub(crate) fn fetch_subscribed_items(
    api_key: &str,
    steamid: u64,
) -> Result<Vec<WorkshopItem>, String> {
    let api_key = api_key.trim();
    if api_key.is_empty() {
        return Err("Add a Steam Web API key in Settings to load subscriptions".to_owned());
    }

    let client = http_client()?;
    let mut items = Vec::new();
    let mut total = u64::MAX;
    let mut page = 1;

    // 50 pages of 100 items is a safety cap far above normal RimWorld use.
    while page <= 50 && (items.len() as u64) < total {
        let input = user_files_input(steamid, page);

        let response = client
            .get(GET_USER_FILES_URL)
            .query(&[("key", api_key), ("input_json", &input.to_string())])
            .send()
            .map_err(|error| format!("Steam API request failed: {error}"))?;

        let status = response.status();
        if !status.is_success() {
            return Err(steam_status_error("subscription", status));
        }

        let envelope = response
            .json::<UserFilesEnvelope>()
            .map_err(|error| format!("Steam API returned an unexpected response: {error}"))?;

        total = envelope.response.total;
        let page_len = envelope.response.publishedfiledetails.len();
        items.extend(
            envelope
                .response
                .publishedfiledetails
                .into_iter()
                .filter_map(PublishedFileDetails::into_item),
        );

        if page_len == 0 {
            break;
        }

        page += 1;
    }

    attach_author_names(api_key, &mut items);

    Ok(items)
}

#[derive(Deserialize)]
struct PlayerSummariesEnvelope {
    response: PlayerSummariesResponse,
}

#[derive(Deserialize)]
struct PlayerSummariesResponse {
    #[serde(default)]
    players: Vec<PlayerSummary>,
}

#[derive(Deserialize)]
struct PlayerSummary {
    steamid: String,
    #[serde(default)]
    personaname: String,
}

/// Fill in creator display names with one batched lookup per page. Author
/// names are a nicety: a failure leaves them blank instead of failing the page.
fn attach_author_names(api_key: &str, items: &mut [WorkshopItem]) {
    let steam_ids = items
        .iter()
        .filter_map(|item| item.author_steam_id)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();

    if steam_ids.is_empty() {
        return;
    }

    let mut names = HashMap::new();

    for chunk in steam_ids.chunks(100) {
        if let Ok(batch) = fetch_player_names(api_key, chunk) {
            names.extend(batch);
        }
    }

    if names.is_empty() {
        return;
    }

    for item in items {
        if let Some(steam_id) = item.author_steam_id {
            item.author = names.get(&steam_id).cloned();
        }
    }
}

fn fetch_player_names(api_key: &str, steam_ids: &[u64]) -> Result<HashMap<u64, String>, String> {
    let client = http_client()?;
    let steam_ids = steam_ids
        .iter()
        .map(u64::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let response = client
        .get(PLAYER_SUMMARIES_URL)
        .query(&[("key", api_key), ("steamids", steam_ids.as_str())])
        .send()
        .map_err(|error| format!("Steam API request failed: {error}"))?;

    let status = response.status();
    if !status.is_success() {
        return Err(steam_status_error("author names", status));
    }

    let envelope = response
        .json::<PlayerSummariesEnvelope>()
        .map_err(|error| format!("Steam API returned an unexpected response: {error}"))?;

    Ok(envelope
        .response
        .players
        .into_iter()
        .filter_map(|player| {
            let steam_id = player.steamid.parse().ok()?;

            Some((steam_id, player.personaname))
        })
        .collect())
}

/// Subscribe or unsubscribe the account that owns the API key.///
/// Steam does not expose subscription changes through the Steamworks SDK to
/// third-party tools, but `IPublishedFileService/Subscribe` authenticates with
/// the user's own Web API key, which is exactly what RimMod already stores.
pub(crate) fn set_subscription(
    api_key: &str,
    published_file_id: u64,
    subscribe: bool,
) -> Result<(), String> {
    let api_key = api_key.trim();
    if api_key.is_empty() {
        return Err("Add a Steam Web API key in Settings to change subscriptions".to_owned());
    }

    let client = http_client()?;
    let url = if subscribe {
        SUBSCRIBE_URL
    } else {
        UNSUBSCRIBE_URL
    };
    let action = if subscribe {
        "subscribe"
    } else {
        "unsubscribe"
    };

    let response = client
        .post(url)
        .form(&[
            ("key", api_key),
            ("publishedfileid", &published_file_id.to_string()),
            ("list_type", "1"),
            ("appid", &RIMWORLD_APP_ID.to_string()),
            ("notify_client", "true"),
        ])
        .send()
        .map_err(|error| format!("Steam API request failed: {error}"))?;

    let status = response.status();
    if !status.is_success() {
        return Err(steam_status_error(action, status));
    }

    Ok(())
}

/// Load subscriptions from the Steam account when possible, otherwise from
/// Steam's local workshop manifest (the file Steam itself keeps for the app).
pub(crate) fn load_subscriptions(
    api_key: Option<&str>,
    workshop_path: Option<&Path>,
) -> Result<SubscriptionSnapshot, String> {
    let mut notes = Vec::new();

    if api_key.is_none() {
        notes.push(
            "add a Steam Web API key in Settings to load subscriptions from your account"
                .to_owned(),
        );
    }

    if let Some(api_key) = api_key.filter(|key| !key.trim().is_empty()) {
        match current_steam_id() {
            Some(steamid) => match fetch_subscribed_items(api_key, steamid) {
                Ok(items) => return Ok(snapshot_from_items(items)),
                Err(error) => notes.push(error),
            },
            None => notes.push("could not find a signed-in Steam account".to_owned()),
        }
    }

    if let Some(workshop_path) = workshop_path {
        match subscribed_ids_from_acf(workshop_path) {
            Ok(ids) => {
                let mut items =
                    fetch_items_by_ids(&ids.iter().copied().collect::<Vec<_>>(), api_key)
                        .unwrap_or_else(|error| {
                            notes.push(error);
                            Vec::new()
                        });

                if items.is_empty() {
                    items = ids.iter().copied().map(placeholder_item).collect();
                }

                return Ok(SubscriptionSnapshot { ids, items });
            }
            Err(error) => notes.push(error),
        }
    } else {
        notes.push("the Workshop folder is not configured".to_owned());
    }

    Err(notes.join(". "))
}

fn snapshot_from_items(items: Vec<WorkshopItem>) -> SubscriptionSnapshot {
    let ids = items
        .iter()
        .map(|item| item.published_file_id)
        .collect::<HashSet<_>>();

    if items.is_empty() {
        SubscriptionSnapshot::default()
    } else {
        SubscriptionSnapshot { ids, items }
    }
}

fn placeholder_item(published_file_id: u64) -> WorkshopItem {
    WorkshopItem {
        published_file_id,
        title: format!("Workshop item {published_file_id}"),
        description: String::new(),
        preview_url: None,
        images: Vec::new(),
        author_steam_id: None,
        author: None,
        subscriptions: None,
        favorites: None,
        time_created: 0,
        time_updated: 0,
        votes_up: 0,
        votes_down: 0,
        tags: Vec::new(),
        supported_versions: Vec::new(),
    }
}

/// Steam locates subscriptions in `steamapps/workshop/appworkshop_294100.acf`.
pub(crate) fn workshop_acf_path(workshop_path: &Path) -> Option<PathBuf> {
    let path = workshop_path.parent()?.parent()?.join(ACF_FILE_NAME);

    path.is_file().then_some(path)
}

pub(crate) fn subscribed_ids_from_acf(workshop_path: &Path) -> Result<HashSet<u64>, String> {
    let path = workshop_acf_path(workshop_path).ok_or_else(|| {
        format!(
            "Steam's Workshop manifest was not found next to {}",
            workshop_path.display()
        )
    })?;

    let contents = fs::read_to_string(&path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;

    Ok(parse_workshop_acf(&contents).into_iter().collect())
}

/// Extract the subscribed IDs from Steam's `appworkshop_<app>.acf`.
///
/// `WorkshopItemDetails` lists items subscribed by the account, while
/// `WorkshopItemsInstalled` only lists downloaded content. SteamCMD writes the
/// latter without any subscription, so it is deliberately ignored.
fn parse_workshop_acf(contents: &str) -> Vec<u64> {
    let mut ids = Vec::new();
    let mut depth = 0usize;
    let mut section = String::new();

    for token in kv_tokens(contents) {
        match token {
            KvToken::Open => depth += 1,
            KvToken::Close => {
                depth = depth.saturating_sub(1);
                if depth <= 1 {
                    section.clear();
                }
            }
            KvToken::Value(value) => match depth {
                1 => section = value,
                2 if section.eq_ignore_ascii_case("WorkshopItemDetails") => {
                    if let Ok(id) = value.parse::<u64>() {
                        ids.push(id);
                    }
                }
                _ => {}
            },
        }
    }

    ids
}

enum KvToken {
    Open,
    Close,
    Value(String),
}

fn kv_tokens(contents: &str) -> Vec<KvToken> {
    let mut tokens = Vec::new();
    let mut characters = contents.chars().peekable();

    while let Some(character) = characters.next() {
        match character {
            '"' => {
                let mut value = String::new();
                while let Some(character) = characters.next() {
                    match character {
                        '"' => break,
                        '\\' => {
                            if let Some(escaped) = characters.next() {
                                value.push(escaped);
                            }
                        }
                        character => value.push(character),
                    }
                }
                tokens.push(KvToken::Value(value));
            }
            '{' => tokens.push(KvToken::Open),
            '}' => tokens.push(KvToken::Close),
            '/' if characters.peek() == Some(&'/') => {
                for character in characters.by_ref() {
                    if character == '\n' {
                        break;
                    }
                }
            }
            _ => {}
        }
    }

    tokens
}

/// The Steam account currently signed in to the local client, used to read its
/// subscription list with the Web API.
pub(crate) fn current_steam_id() -> Option<u64> {
    let steam_dir = steamlocate::locate().ok()?;
    let path = steam_dir.path().join("config").join("loginusers.vdf");
    let contents = fs::read_to_string(path).ok()?;

    parse_most_recent_steam_id(&contents)
}

fn parse_most_recent_steam_id(contents: &str) -> Option<u64> {
    #[derive(Default)]
    struct Account {
        id: u64,
        most_recent: bool,
        auto_login: bool,
        timestamp: u64,
    }

    let mut accounts = Vec::new();
    let mut current = None;

    for line in contents.lines() {
        let values = quoted_values(line);

        match values.as_slice() {
            [only] if is_steam_id(only) => {
                let Ok(id) = only.parse() else {
                    continue;
                };

                accounts.push(Account {
                    id,
                    ..Account::default()
                });
                current = Some(accounts.len() - 1);
            }
            [key, value, ..] => {
                let Some(index) = current else {
                    continue;
                };

                match key.to_ascii_lowercase().as_str() {
                    "mostrecent" => accounts[index].most_recent = *value == "1",
                    "autologin" => accounts[index].auto_login = *value == "1",
                    "timestamp" => accounts[index].timestamp = value.parse().unwrap_or(0),
                    _ => {}
                }
            }
            _ => {}
        }
    }

    // Steam stopped writing MostRecent in newer clients. Prefer the explicitly
    // flagged account, then the auto-login account, then the newest session.
    accounts
        .iter()
        .find(|account| account.most_recent)
        .or_else(|| {
            accounts
                .iter()
                .filter(|account| account.auto_login)
                .max_by_key(|account| account.timestamp)
        })
        .or_else(|| accounts.iter().max_by_key(|account| account.timestamp))
        .map(|account| account.id)
}

fn is_steam_id(value: &str) -> bool {
    value.len() == 17 && value.chars().all(|character| character.is_ascii_digit())
}

fn quoted_values(line: &str) -> Vec<&str> {
    let mut values = Vec::new();
    let mut rest = line;

    while let Some(start) = rest.find('"') {
        let after_start = &rest[start + 1..];
        let Some(end) = after_start.find('"') else {
            break;
        };

        values.push(&after_start[..end]);
        rest = &after_start[end + 1..];
    }

    values
}

const STEAM_PREVIEW_TYPE_IMAGE: u64 = 0;

/// Collect image URLs from Steam's item `previews` array in author-defined
/// display order, skipping non-image entries such as YouTube videos.
fn extract_preview_image_urls(previews: &[SteamPreview]) -> Vec<String> {
    let mut entries: Vec<(u64, usize, String)> = previews
        .iter()
        .enumerate()
        .filter(|(_, preview)| {
            preview.preview_type.unwrap_or(STEAM_PREVIEW_TYPE_IMAGE) == STEAM_PREVIEW_TYPE_IMAGE
        })
        .filter_map(|(index, preview)| {
            let url = bbcode::normalize_image_url(&preview.url)?;
            Some((preview.sortorder.unwrap_or(u64::MAX), index, url))
        })
        .collect();

    entries.sort_by_key(|(sort_order, index, _)| (*sort_order, *index));

    let mut urls = Vec::with_capacity(entries.len());
    for (_, _, url) in entries {
        if !urls.contains(&url) {
            urls.push(url);
        }
    }

    urls
}

pub(crate) fn format_count(value: u64) -> String {
    fn scaled(value: u64, divisor: f64, suffix: &str) -> String {
        let scaled = value as f64 / divisor;

        if scaled >= 100.0 {
            format!("{scaled:.0}{suffix}")
        } else {
            let text = format!("{scaled:.1}{suffix}");
            text.replace(".0", "")
        }
    }

    if value >= 1_000_000 {
        scaled(value, 1_000_000.0, "M")
    } else if value >= 1_000 {
        scaled(value, 1_000.0, "k")
    } else {
        value.to_string()
    }
}

/// Format a Unix timestamp as `YYYY-MM-DD` without pulling in a date crate.
pub(crate) fn format_date(timestamp: u64) -> Option<String> {
    let days = i64::try_from(timestamp / 86_400).ok()?;
    let (year, month, day) = civil_from_days(days);

    Some(format!("{year:04}-{month:02}-{day:02}"))
}

/// Howard Hinnant's `civil_from_days`.
fn civil_from_days(days_since_epoch: i64) -> (i64, u32, u32) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_prime + 2) / 5 + 1) as u32;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    } as u32;

    (year + i64::from(month <= 2), month, day)
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

/// Whether a usable SteamCMD executable is configured or on `PATH`.
pub(crate) fn steamcmd_available(configured_steamcmd_path: Option<&Path>) -> bool {
    let steamcmd_path = configured_steamcmd_path
        .map(Path::to_path_buf)
        .unwrap_or_else(default_steamcmd_path);

    resolve_executable(&steamcmd_path).is_some()
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
