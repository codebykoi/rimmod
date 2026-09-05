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
