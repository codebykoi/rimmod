use super::*;

#[test]
fn text_search_uses_relevance_ranking() {
    assert_eq!(WorkshopSort::Popular.query_type(true), 12);
    assert_eq!(WorkshopSort::Updated.query_type(true), 12);
}

#[test]
fn empty_search_uses_selected_ranking() {
    assert_eq!(WorkshopSort::Popular.query_type(false), 0);
    assert_eq!(WorkshopSort::MostSubscribed.query_type(false), 9);
    assert_eq!(WorkshopSort::Recent.query_type(false), 1);
    assert_eq!(WorkshopSort::Updated.query_type(false), 21);
}

#[test]
fn workshop_page_url_contains_item_id() {
    let item = WorkshopItem {
        published_file_id: 123,
        title: String::new(),
        description: String::new(),
        preview_url: None,
        subscriptions: None,
        supported_versions: Vec::new(),
    };

    assert_eq!(
        item.page_url(),
        "https://steamcommunity.com/sharedfiles/filedetails/?id=123"
    );
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
                    "short_description": "An example",
                    "preview_url": "https://example.com/preview.png",
                    "subscriptions": "9001"
                }]
            }
        }
    "#;

    let envelope: QueryFilesEnvelope = serde_json::from_str(json).expect("valid test response");
    let details = &envelope.response.publishedfiledetails[0];

    assert_eq!(envelope.response.total, 42);
    assert_eq!(details.publishedfileid, "123");
    assert_eq!(details.subscriptions, Some(9001));
}

#[test]
fn query_response_requires_the_total_field() {
    let json = r#"{"response":{"result":0}}"#;

    assert!(serde_json::from_str::<QueryFilesEnvelope>(json).is_err());
}

#[test]
fn version_tags_become_sorted_supported_versions() {
    let tags = ["Mod", "1.6", "1.5", "1.4", "not a version", "1.4"]
        .map(|tag| SteamTag {
            tag: tag.to_owned(),
        })
        .into();

    assert_eq!(extract_supported_versions(tags), ["1.4", "1.5", "1.6"]);
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
