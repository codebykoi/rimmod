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
    assert_eq!(input["return_previews"], true);
}

#[test]
fn user_files_and_details_requests_include_additional_previews() {
    let user_input = user_files_input(76_561_198_000_000_000, 1);
    assert_eq!(user_input["return_previews"], true);

    let details_params = details_query_params("secret", &[100, 200]);
    assert!(details_params.contains(&("includeadditionalpreviews".to_owned(), "true".to_owned())));
    assert!(details_params.contains(&("includetags".to_owned(), "true".to_owned())));
    assert!(details_params.contains(&("includevotes".to_owned(), "true".to_owned())));
    assert!(details_params.contains(&("publishedfileids[0]".to_owned(), "100".to_owned())));
    assert!(details_params.contains(&("publishedfileids[1]".to_owned(), "200".to_owned())));
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
fn item_images_come_from_previews_not_description() {
    let json = r#"
        {
            "publishedfileid": "123",
            "title": "Vanilla Vehicles Expanded - Tier 3",
            "file_description": "[img]https://example.com/patreon-banner.png[/img]",
            "preview_url": "https://example.com/cover.png",
            "previews": [
                {
                    "previewid": "3",
                    "sortorder": 20,
                    "url": "//steamuserimages-a.akamaihd.net/ugc/second.jpg",
                    "preview_type": 0
                },
                {
                    "previewid": "2",
                    "sortorder": 15,
                    "youtubevideoid": "dQw4w9WgXcQ",
                    "preview_type": 1
                },
                {
                    "previewid": "1",
                    "sortorder": "10",
                    "url": "https://steamuserimages-a.akamaihd.net/ugc/first.jpg",
                    "preview_type": 0
                },
                {
                    "previewid": "4",
                    "sortorder": 30,
                    "url": "https://steamuserimages-a.akamaihd.net/ugc/first.jpg",
                    "preview_type": 0
                },
                {
                    "previewid": "5",
                    "sortorder": 40,
                    "url": "not-a-url",
                    "preview_type": 0
                }
            ]
        }
    "#;

    let details: PublishedFileDetails = serde_json::from_str(json).expect("valid details");
    let item = details.into_item().expect("item");

    assert_eq!(
        item.images,
        [
            "https://steamuserimages-a.akamaihd.net/ugc/first.jpg",
            "https://steamuserimages-a.akamaihd.net/ugc/second.jpg",
        ]
    );
}

#[test]
fn preview_slideshow_starts_with_cover_and_deduplicates_previews() {
    let mut item = sample_item();
    item.preview_url = Some("https://example.com/cover.png".to_owned());
    item.images = vec![
        "https://example.com/cover.png".to_owned(),
        "https://example.com/shot1.png".to_owned(),
        "https://example.com/shot2.png".to_owned(),
    ];

    assert_eq!(
        item.preview_slideshow_urls(),
        [
            "https://example.com/cover.png",
            "https://example.com/shot1.png",
            "https://example.com/shot2.png",
        ]
    );

    item.preview_url = None;
    assert_eq!(
        item.preview_slideshow_urls(),
        [
            "https://example.com/cover.png",
            "https://example.com/shot1.png",
            "https://example.com/shot2.png",
        ]
    );
}

#[test]
fn bbcode_parses_headings_styles_links_lists_and_keeps_unknown_tags() {
    let description = "[h1]Introduction[/h1]\n\
        [b]DMS2[/b] and [url=https://example.com]a link[/url] with [color=red]red[/color]\n\
        [list]\n\
        [*][b]Harmony[/b]\n\
        [*]Biotech DLC[/*]\n\
        [/list]\n\
        [noparse][b]literal[/b][/noparse]";

    let blocks = bbcode::parse_bbcode(description);

    assert_eq!(
        blocks,
        vec![
            bbcode::DescriptionBlock::Heading {
                level: 1,
                spans: vec![bbcode::DescriptionSpan {
                    text: "Introduction".to_owned(),
                    style: bbcode::SpanStyle::default(),
                    link: None,
                }],
            },
            bbcode::DescriptionBlock::Paragraph(vec![
                bbcode::DescriptionSpan {
                    text: "DMS2".to_owned(),
                    style: bbcode::SpanStyle {
                        bold: true,
                        ..Default::default()
                    },
                    link: None,
                },
                bbcode::DescriptionSpan {
                    text: " and ".to_owned(),
                    style: bbcode::SpanStyle::default(),
                    link: None,
                },
                bbcode::DescriptionSpan {
                    text: "a link".to_owned(),
                    style: bbcode::SpanStyle::default(),
                    link: Some("https://example.com".to_owned()),
                },
                bbcode::DescriptionSpan {
                    text: " with [color=red]red[/color]".to_owned(),
                    style: bbcode::SpanStyle::default(),
                    link: None,
                },
            ]),
            bbcode::DescriptionBlock::List {
                ordered: false,
                items: vec![
                    vec![bbcode::DescriptionBlock::Paragraph(vec![
                        bbcode::DescriptionSpan {
                            text: "Harmony".to_owned(),
                            style: bbcode::SpanStyle {
                                bold: true,
                                ..Default::default()
                            },
                            link: None,
                        },
                    ])],
                    vec![bbcode::DescriptionBlock::Paragraph(vec![
                        bbcode::DescriptionSpan {
                            text: "Biotech DLC".to_owned(),
                            style: bbcode::SpanStyle::default(),
                            link: None,
                        },
                    ])],
                ],
            },
            bbcode::DescriptionBlock::Paragraph(vec![bbcode::DescriptionSpan {
                text: "[b]literal[/b]".to_owned(),
                style: bbcode::SpanStyle::default(),
                link: None,
            }]),
        ]
    );
}

#[test]
fn bbcode_parses_linked_images_image_rows_and_tables() {
    let description = "[url=https://steamcommunity.com/id/author][img]https://example.com/banner.png[/img][/url]\n\
        [img]//example.com/card1.png[/img] [img]https://example.com/card2.png[/img]\n\
        [table noborder=1]\n\
        [tr]\n\
        [td][img]https://example.com/left.png[/img][/td]\n\
        [td][img]https://example.com/right1.png[/img]\n\
        [img]https://example.com/right2.png[/img][/td]\n\
        [/tr]\n\
        [/table]";

    let blocks = bbcode::parse_bbcode(description);

    assert_eq!(
        blocks,
        vec![
            bbcode::DescriptionBlock::ImageRow(vec![bbcode::DescriptionImage {
                url: "https://example.com/banner.png".to_owned(),
                link: Some("https://steamcommunity.com/id/author".to_owned()),
            }]),
            bbcode::DescriptionBlock::ImageRow(vec![
                bbcode::DescriptionImage {
                    url: "https://example.com/card1.png".to_owned(),
                    link: None,
                },
                bbcode::DescriptionImage {
                    url: "https://example.com/card2.png".to_owned(),
                    link: None,
                },
            ]),
            bbcode::DescriptionBlock::Table {
                bordered: false,
                rows: vec![vec![
                    bbcode::TableCell {
                        header: false,
                        blocks: vec![bbcode::DescriptionBlock::ImageRow(vec![
                            bbcode::DescriptionImage {
                                url: "https://example.com/left.png".to_owned(),
                                link: None,
                            },
                        ])],
                    },
                    bbcode::TableCell {
                        header: false,
                        blocks: vec![
                            bbcode::DescriptionBlock::ImageRow(vec![bbcode::DescriptionImage {
                                url: "https://example.com/right1.png".to_owned(),
                                link: None,
                            }]),
                            bbcode::DescriptionBlock::ImageRow(vec![bbcode::DescriptionImage {
                                url: "https://example.com/right2.png".to_owned(),
                                link: None,
                            }]),
                        ],
                    },
                ]],
            },
        ]
    );
}

#[test]
fn bbcode_parses_quotes_code_dividers_youtube_and_bare_urls() {
    let description = "[quote=Oskar]Mod details[/quote]\n\
        [hr][/hr]\n\
        [code]let x = [b]raw[/b];[/code]\n\
        [previewyoutube=dQw4w9WgXcQ;full][/previewyoutube]\n\
        [url]www.patreon.com/author[/url]";

    let blocks = bbcode::parse_bbcode(description);

    assert_eq!(
        blocks,
        vec![
            bbcode::DescriptionBlock::Quote {
                author: Some("Oskar".to_owned()),
                blocks: vec![bbcode::DescriptionBlock::Paragraph(vec![
                    bbcode::DescriptionSpan {
                        text: "Mod details".to_owned(),
                        style: bbcode::SpanStyle::default(),
                        link: None,
                    },
                ])],
            },
            bbcode::DescriptionBlock::Divider,
            bbcode::DescriptionBlock::Code("let x = [b]raw[/b];".to_owned()),
            bbcode::DescriptionBlock::YouTube {
                video_id: "dQw4w9WgXcQ".to_owned(),
            },
            bbcode::DescriptionBlock::Paragraph(vec![bbcode::DescriptionSpan {
                text: "www.patreon.com/author".to_owned(),
                style: bbcode::SpanStyle::default(),
                link: Some("https://www.patreon.com/author".to_owned()),
            }]),
        ]
    );
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
