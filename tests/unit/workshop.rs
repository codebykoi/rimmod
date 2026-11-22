use super::*;

fn sample_item() -> WorkshopItem {
    WorkshopItem {
        published_file_id: 123,
        title: "Example mod".to_owned(),
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

#[test]
fn text_search_uses_relevance_ranking() {
    assert_eq!(WorkshopSort::Relevance.query_type(), 12);
}

#[test]
fn empty_search_uses_selected_ranking() {
    assert_eq!(WorkshopSort::Trending.query_type(), 3);
    assert_eq!(WorkshopSort::TopRated.query_type(), 11);
    assert_eq!(WorkshopSort::MostSubscribed.query_type(), 9);
    assert_eq!(WorkshopSort::Recent.query_type(), 1);
    assert_eq!(WorkshopSort::Updated.query_type(), 21);
}

#[test]
fn query_input_combines_version_and_dlc_filters() {
    let query = WorkshopQuery {
        search_text: String::new(),
        sort: WorkshopSort::Trending,
        page: 2,
        versions: vec!["1.6".to_owned()],
        dlc: vec!["Biotech".to_owned()],
    };

    let input = query_input(&query);

    assert_eq!(input["query_type"], 3);
    assert_eq!(input["page"], 2);
    assert_eq!(input["requiredtags"][0], "1.6");
    assert_eq!(input["requiredtags"][1], "Biotech");
    assert_eq!(input["match_all_tags"], true);
    assert_eq!(input["search_text"], "");
}

#[test]
fn query_input_uses_the_selected_sort_with_search_text() {
    let query = WorkshopQuery {
        search_text: "  vanilla  ".to_owned(),
        sort: WorkshopSort::Recent,
        page: 1,
        versions: Vec::new(),
        dlc: Vec::new(),
    };

    let input = query_input(&query);

    assert_eq!(input["query_type"], 1);
    assert_eq!(input["search_text"], "vanilla");
    assert_eq!(input["requiredtags"].as_array().map(Vec::len), Some(0));
}

#[test]
fn query_input_can_request_relevance() {
    let query = WorkshopQuery {
        search_text: "vanilla".to_owned(),
        sort: WorkshopSort::Relevance,
        page: 1,
        versions: Vec::new(),
        dlc: Vec::new(),
    };

    assert_eq!(query_input(&query)["query_type"], 12);
}

#[test]
fn workshop_page_url_contains_item_id() {
    assert_eq!(
        sample_item().page_url(),
        "https://steamcommunity.com/sharedfiles/filedetails/?id=123"
    );
    assert_eq!(
        sample_item().steam_url(),
        "steam://url/CommunityFilePage/123"
    );
}

#[test]
fn rating_is_the_share_of_positive_votes() {
    let mut item = sample_item();
    item.votes_up = 90;
    item.votes_down = 10;

    assert_eq!(item.rating_percent(), Some(90));

    item.votes_up = 0;
    item.votes_down = 0;
    assert_eq!(item.rating_percent(), None);
}

#[test]
fn query_response_accepts_steam_string_numbers() {
    let json = r#"
        {
            "response": {
                "total": 42,
                "publishedfiledetails": [{
                    "publishedfileid": "123",
                    "title": "Example mod",
                    "creator": "76561198000000000",
                    "file_description": "An example",
                    "preview_url": "https://example.com/preview.png",
                    "subscriptions": "9001",
                    "favorited": 12,
                    "time_updated": "1700000000",
                    "vote_data": { "votes_up": "90", "votes_down": 10 }
                }]
            }
        }
    "#;

    let envelope: QueryFilesEnvelope = serde_json::from_str(json).expect("valid test response");

    assert_eq!(envelope.response.total, 42);
    let details = &envelope.response.publishedfiledetails[0];
    assert_eq!(details.publishedfileid, "123");
    assert_eq!(details.description, "An example");
    assert_eq!(details.subscriptions, Some(9001));
    assert_eq!(details.favorited, Some(12));
    assert_eq!(details.time_updated, Some(1_700_000_000));
    assert_eq!(details.creator, "76561198000000000");

    let item = envelope
        .response
        .publishedfiledetails
        .into_iter()
        .next()
        .and_then(PublishedFileDetails::into_item)
        .expect("item");
    assert_eq!(item.rating_percent(), Some(90));
    assert_eq!(item.author_steam_id, Some(76_561_198_000_000_000));
}

#[test]
fn legacy_description_field_is_accepted() {
    let json = r#"{
        "publishedfileid": "5",
        "description": "Legacy description"
    }"#;

    let details: PublishedFileDetails = serde_json::from_str(json).expect("valid details");

    assert_eq!(details.description, "Legacy description");
}

#[test]
fn query_response_requires_the_total_field() {
    let json = r#"{"response":{"result":0}}"#;

    assert!(serde_json::from_str::<QueryFilesEnvelope>(json).is_err());
}

#[test]
fn version_tags_become_sorted_supported_versions() {
    let tags = ["Mod", "1.6", "1.5", "1.4", "not a version", "1.4"]
        .map(str::to_owned)
        .to_vec();

    assert_eq!(extract_supported_versions(&tags), ["1.4", "1.5", "1.6"]);
}

#[test]
fn response_without_tags_leaves_supported_versions_empty() {
    let json = r#"
        {
            "response": {
                "total": 1,
                "publishedfiledetails": [{
                    "publishedfileid": "123",
                    "title": "Example mod"
                }]
            }
        }
    "#;

    let envelope: QueryFilesEnvelope = serde_json::from_str(json).expect("valid test response");

    assert!(envelope.response.publishedfiledetails[0].tags.is_empty());
}

#[test]
fn response_tags_are_deserialized() {
    let json = r#"
        {
            "response": {
                "total": 1,
                "publishedfiledetails": [{
                    "publishedfileid": "123",
                    "title": "Example mod",
                    "tags": [{"tag": "Mod"}, {"tag": "1.6"}, {"tag": "1.5"}]
                }]
            }
        }
    "#;

    let envelope: QueryFilesEnvelope = serde_json::from_str(json).expect("valid test response");
    let details = &envelope.response.publishedfiledetails[0];

    assert_eq!(details.tags.len(), 3);
    assert_eq!(details.tags[1].tag, "1.6");
}

const SAMPLE_ACF: &str = r#"
"AppWorkshop"
{
    "appid"        "294100"
    "WorkshopItemsInstalled"
    {
        "111111"
        {
            "size"          "10"
            "timeupdated"   "1700000000"
        }
    }
    "WorkshopItemDetails"
    {
        "222222"
        {
            // Subscribed by the local account.
            "manifest"      "333333"
            "subscribedby"  "76561198000000000"
        }
        "444444"
        {
            "manifest"      "555555"
            "subscribedby"  "76561198000000000"
        }
    }
}
"#;

#[test]
fn acf_subscriptions_come_from_item_details_only() {
    assert_eq!(parse_workshop_acf(SAMPLE_ACF), [222_222, 444_444]);
}

#[test]
fn acf_without_item_details_has_no_subscriptions() {
    let acf = r#"
        "AppWorkshop"
        {
            "WorkshopItemsInstalled"
            {
                "111111" { "size" "10" }
            }
        }
    "#;

    assert!(parse_workshop_acf(acf).is_empty());
}

#[test]
fn most_recent_steam_id_is_read_from_loginusers() {
    let loginusers = r#"
"users"
{
    "76561198000000001"
    {
        "AccountName"   "first"
        "MostRecent"    "0"
        "Timestamp"     "1700000000"
    }
    "76561198000000002"
    {
        "AccountName"   "second"
        "MostRecent"    "1"
        "Timestamp"     "1700000001"
    }
}
"#;

    assert_eq!(
        parse_most_recent_steam_id(loginusers),
        Some(76_561_198_000_000_002)
    );
}

#[test]
fn loginusers_falls_back_to_the_auto_login_account() {
    let loginusers = r#"
"users"
{
    "76561198000000001"
    {
        "AccountName"   "first"
        "AutoLogin"     "0"
        "timestamp"     "1700000002"
    }
    "76561198000000002"
    {
        "AccountName"   "second"
        "AutoLogin"     "1"
        "timestamp"     "1700000001"
    }
}
"#;

    assert_eq!(
        parse_most_recent_steam_id(loginusers),
        Some(76_561_198_000_000_002)
    );
}

#[test]
fn loginusers_with_one_account_returns_it() {
    let loginusers = r#"
"users"
{
    "76561198000000001"
    {
        "AccountName"   "only"
    }
}
"#;

    assert_eq!(
        parse_most_recent_steam_id(loginusers),
        Some(76_561_198_000_000_001)
    );
}

#[test]
fn description_images_are_extracted_for_the_gallery() {
    let description = "Gallery:\n\
        [img]https://i.imgur.com/a.png[/img]\n\
        [IMG]//i.imgur.com/b.jpg[/IMG]\n\
        [img]https://i.imgur.com/a.png[/img]\n\
        [img]not-a-url[/img]\n\
        [url=https://example.com]a link[/url]";

    assert_eq!(
        extract_image_urls(description),
        ["https://i.imgur.com/a.png", "https://i.imgur.com/b.jpg"]
    );
}

#[test]
fn image_gallery_stops_after_twelve_entries() {
    let description = (0..20)
        .map(|index| format!("[img]https://example.com/{index}.png[/img]"))
        .collect::<String>();

    assert_eq!(extract_image_urls(&description).len(), 12);
}

#[test]
fn bbcode_is_stripped_but_unknown_tags_are_kept() {
    let description = "[b]Bold[/b] and [url=https://example.com]a link[/url]\n\
                       [list]\n[*]one\n[/list]\n\
                       [img]https://example.com/image.png[/img]\n\
                       [color=red]red[/color]";

    let stripped = strip_bbcode(description);

    assert!(stripped.contains("Bold and a link"));
    assert!(stripped.contains("• one"));
    assert!(!stripped.contains("[url"));
    assert!(!stripped.contains("image.png"));
    assert!(stripped.contains("[color=red]red[/color]"));
}

#[test]
fn counts_are_shortened_for_cards() {
    assert_eq!(format_count(42), "42");
    assert_eq!(format_count(1_000), "1k");
    assert_eq!(format_count(1_500), "1.5k");
    assert_eq!(format_count(2_452_000), "2.5M");
    assert_eq!(format_count(10_000_000), "10M");
}

#[test]
fn unix_timestamps_are_formatted_as_dates() {
    assert_eq!(format_date(0).as_deref(), Some("1970-01-01"));
    assert_eq!(format_date(1_752_434_318).as_deref(), Some("2025-07-13"));
}

#[test]
fn steamcmd_failure_text_is_detected_even_with_a_zero_exit_code() {
    assert!(steamcmd_output_reports_failure(
        "ERROR! Download item 123 failed (Failure).",
        ""
    ));
    assert!(!steamcmd_output_reports_failure(
        "Success. Downloaded item 123",
        ""
    ));
}

#[test]
fn success_markers_match_only_the_requested_item() {
    let output = "Success. Downloaded item 123 to \"workshop/content/294100/123\" (10 bytes)\n\
                  Success. Downloaded item 1234 to \"workshop/content/294100/1234\" (10 bytes)";

    assert!(contains_item_marker(
        &output.to_ascii_lowercase(),
        "success. downloaded item ",
        123
    ));
    assert!(contains_item_marker(
        &output.to_ascii_lowercase(),
        "success. downloaded item ",
        1234
    ));
    assert!(!contains_item_marker(
        &output.to_ascii_lowercase(),
        "success. downloaded item ",
        12
    ));
    assert!(!contains_item_marker(
        &output.to_ascii_lowercase(),
        "success. downloaded item ",
        999
    ));
}

#[test]
fn failure_markers_match_only_the_requested_item() {
    let output = "ERROR! Download item 123 failed (Timeout).";

    assert!(contains_item_marker(
        &output.to_ascii_lowercase(),
        "error! download item ",
        123
    ));
    assert!(!contains_item_marker(
        &output.to_ascii_lowercase(),
        "error! download item ",
        1234
    ));
    assert!(!contains_item_marker(
        &output.to_ascii_lowercase(),
        "error! download item ",
        1
    ));
}

#[test]
fn item_outcomes_ignore_success_lines_of_other_items() {
    let output = "Success. Downloaded item 100\nERROR! Download item 200 failed (Failure).";
    let lowercase = output.to_ascii_lowercase();

    assert!(contains_item_marker(
        &lowercase,
        "success. downloaded item ",
        100
    ));
    assert!(!contains_item_marker(
        &lowercase,
        "success. downloaded item ",
        200
    ));
    assert!(contains_item_marker(
        &lowercase,
        "error! download item ",
        200
    ));
    assert!(!contains_item_marker(
        &lowercase,
        "error! download item ",
        100
    ));
}
