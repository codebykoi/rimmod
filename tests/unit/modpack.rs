use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use super::*;

fn test_directory(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("rimmod-modpack-{}-{name}", std::process::id()));

    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).expect("create test directory");

    path
}

fn write_file(path: &Path, contents: &[u8]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent directory");
    }

    fs::write(path, contents).expect("write test file");
}

fn local_manifest(folder_name: &str) -> ModpackManifest {
    let mut manifest = ModpackManifest::new("Test pack");
    manifest.description = "A pack used by the unit tests".to_owned();
    manifest.tags = vec!["Testing".to_owned(), "Round trip".to_owned()];
    manifest.game_version = Some("1.6.4566".to_owned());
    manifest.mods = vec![
        PackMod {
            package_id: "author.localmod".to_owned(),
            name: "Local mod".to_owned(),
            folder_name: folder_name.to_owned(),
            source: PackSource::Local {
                remote_url: None,
                path: format!("mods/{folder_name}"),
            },
        },
        PackMod {
            package_id: "author.workshopmod".to_owned(),
            name: "Workshop mod".to_owned(),
            folder_name: String::new(),
            source: PackSource::Workshop {
                workshop_id: 123_456,
            },
        },
        PackMod {
            package_id: "ludeon.rimworld".to_owned(),
            name: "Core".to_owned(),
            folder_name: String::new(),
            source: PackSource::Official,
        },
    ];
    manifest.organization.insert(
        "author.localmod".to_owned(),
        PackOrganization {
            tags: vec!["Fixes".to_owned()],
            color: Some(ModColor::Teal),
        },
    );

    manifest
}

#[test]
fn instance_round_trips_through_an_archive() {
    let directory = test_directory("round-trip");
    let instance = directory.join("instance");
    let exported = directory.join("exported.rimpack");
    let imported = directory.join("imported");
    let mod_folder = directory.join("source-mod");
    let config_root = directory.join("Config");
    let saves_root = directory.join("Saves");
    let save_path = saves_root.join("My Save.rws");

    write_file(
        &mod_folder.join("About").join("About.xml"),
        b"<ModMetaData/>",
    );
    write_file(
        &mod_folder.join("Textures").join("preview.png"),
        b"texture-data",
    );
    write_file(&config_root.join("Mod_TestMod.xml"), b"<settings/>");
    write_file(
        &config_root.join("HugsLib").join("ModSettings.xml"),
        b"<hugs/>",
    );
    write_file(&save_path, b"save-data");

    let configs = discover_config_files(&config_root);
    let saves = vec![("My Save.rws".to_owned(), save_path)];

    let manifest = local_manifest("LocalMod");
    let icon: &[u8] = b"fake-icon-bytes";

    write_instance(
        &instance,
        PackWriteRequest {
            manifest: &manifest,
            icon: Some(icon),
            local_mods: &[("LocalMod".to_owned(), mod_folder)],
            configs: &configs,
            saves: &saves,
        },
        &mut |_| {},
    )
    .expect("write instance");

    let loaded = read_instance(&instance).expect("read instance");

    assert_eq!(loaded.manifest.name, "Test pack");
    assert_eq!(loaded.manifest.mods.len(), 3);
    assert_eq!(
        loaded.manifest.configs,
        ["HugsLib/ModSettings.xml", "Mod_TestMod.xml"]
    );
    assert_eq!(loaded.manifest.saves, ["My Save.rws"]);
    assert_eq!(loaded.icon.as_deref(), Some(icon));

    export_instance(&instance, &exported, &mut |_| {}).expect("export instance");
    import_archive(&exported, &imported, &mut |_| {}).expect("import archive");

    let imported_pack = read_instance(&imported).expect("read imported instance");

    assert_eq!(imported_pack.manifest.name, "Test pack");
    assert_eq!(
        imported_pack.manifest.configs,
        ["HugsLib/ModSettings.xml", "Mod_TestMod.xml"]
    );
    assert_eq!(imported_pack.manifest.saves, ["My Save.rws"]);
    assert_eq!(
        fs::read(imported.join("saves").join("My Save.rws")).expect("read imported save"),
        b"save-data"
    );
    assert_eq!(
        fs::read(
            imported
                .join("mods")
                .join("LocalMod")
                .join("Textures")
                .join("preview.png")
        )
        .expect("read imported mod file"),
        b"texture-data"
    );

    fs::remove_dir_all(directory).expect("remove test directory");
}

#[test]
fn writing_an_instance_twice_replaces_it() {
    let directory = test_directory("replace-instance");
    let instance = directory.join("instance");
    let mod_folder = directory.join("source-mod");

    write_file(
        &mod_folder.join("About").join("About.xml"),
        b"<ModMetaData/>",
    );

    let first = ModpackManifest::new("First");
    write_instance(
        &instance,
        PackWriteRequest {
            manifest: &first,
            icon: None,
            local_mods: &[("LocalMod".to_owned(), mod_folder.clone())],
            configs: &[],
            saves: &[],
        },
        &mut |_| {},
    )
    .expect("write first instance");

    let second = ModpackManifest::new("Second");
    write_instance(
        &instance,
        PackWriteRequest {
            manifest: &second,
            icon: None,
            local_mods: &[("LocalMod".to_owned(), mod_folder)],
            configs: &[],
            saves: &[],
        },
        &mut |_| {},
    )
    .expect("replace instance");

    let loaded = read_instance(&instance).expect("read replaced instance");

    assert_eq!(loaded.manifest.name, "Second");

    fs::remove_dir_all(directory).expect("remove test directory");
}

#[test]
fn applying_an_instance_copies_mods_settings_and_saves() {
    let directory = test_directory("apply");
    let instance = directory.join("instance");
    let mod_folder = directory.join("source-mod");
    let source_config = directory.join("source-config");
    let source_saves = directory.join("source-saves");
    let mods_root = directory.join("RimWorld").join("Mods");
    let config_root = directory.join("RimWorld").join("Config");
    let saves_root = directory.join("RimWorld").join("Saves");

    write_file(
        &mod_folder.join("About").join("About.xml"),
        b"<ModMetaData>local</ModMetaData>",
    );
    write_file(&mod_folder.join("Defs").join("ThingDefs.xml"), b"<Defs/>");
    write_file(
        &source_config.join("Mod_TestMod.xml"),
        b"<settings>source</settings>",
    );
    write_file(&source_saves.join("Progress.rws"), b"<save>progress</save>");

    let manifest = local_manifest("LocalMod");
    let configs = discover_config_files(&source_config);
    let saves = vec![("Progress.rws".to_owned(), source_saves.join("Progress.rws"))];

    write_instance(
        &instance,
        PackWriteRequest {
            manifest: &manifest,
            icon: None,
            local_mods: &[("LocalMod".to_owned(), mod_folder)],
            configs: &configs,
            saves: &saves,
        },
        &mut |_| {},
    )
    .expect("write instance");

    let applied = apply_instance(
        &instance,
        &mods_root,
        &config_root,
        &saves_root,
        false,
        &mut |_| {},
    )
    .expect("apply instance");

    assert_eq!(applied.installed_local_mods, ["LocalMod"]);
    assert!(applied.skipped_local_mods.is_empty());
    assert_eq!(applied.restored_configs, ["Mod_TestMod.xml"]);
    assert_eq!(applied.restored_saves, ["Progress.rws"]);
    assert_eq!(
        fs::read(mods_root.join("LocalMod").join("About").join("About.xml"))
            .expect("read extracted about"),
        b"<ModMetaData>local</ModMetaData>"
    );
    assert_eq!(
        fs::read(config_root.join("Mod_TestMod.xml")).expect("read restored settings"),
        b"<settings>source</settings>"
    );
    assert_eq!(
        fs::read(saves_root.join("Progress.rws")).expect("read restored save"),
        b"<save>progress</save>"
    );

    fs::remove_dir_all(directory).expect("remove test directory");
}

#[test]
fn existing_saves_are_kept_unless_overwrite_is_requested() {
    let directory = test_directory("save-conflict");
    let instance = directory.join("instance");
    let source_saves = directory.join("source-saves");
    let save_path = source_saves.join("Progress.rws");
    let mods_root = directory.join("Mods");
    let config_root = directory.join("Config");
    let saves_root = directory.join("Saves");

    write_file(&save_path, b"pack version");

    let manifest = local_manifest("LocalMod");
    let saves = vec![("Progress.rws".to_owned(), save_path)];

    write_instance(
        &instance,
        PackWriteRequest {
            manifest: &manifest,
            icon: None,
            local_mods: &[],
            configs: &[],
            saves: &saves,
        },
        &mut |_| {},
    )
    .expect("write instance");

    apply_instance(
        &instance,
        &mods_root,
        &config_root,
        &saves_root,
        false,
        &mut |_| {},
    )
    .expect("first apply");

    write_file(&saves_root.join("Progress.rws"), b"newer local progress");

    let kept = apply_instance(
        &instance,
        &mods_root,
        &config_root,
        &saves_root,
        false,
        &mut |_| {},
    )
    .expect("apply without overwrite");

    assert!(kept.restored_saves.is_empty());
    assert_eq!(kept.skipped_saves, ["Progress.rws"]);
    assert_eq!(
        fs::read(saves_root.join("Progress.rws")).expect("read kept save"),
        b"newer local progress"
    );

    let replaced = apply_instance(
        &instance,
        &mods_root,
        &config_root,
        &saves_root,
        true,
        &mut |_| {},
    )
    .expect("apply with overwrite");

    assert_eq!(replaced.restored_saves, ["Progress.rws"]);
    assert!(replaced.skipped_saves.is_empty());
    assert_eq!(
        fs::read(saves_root.join("Progress.rws")).expect("read replaced save"),
        b"pack version"
    );

    fs::remove_dir_all(directory).expect("remove test directory");
}

#[test]
fn archive_entries_cannot_escape_the_instance_folder() {
    let directory = test_directory("traversal");
    let archive_path = directory.join("evil.rimpack");
    let instance = directory.join("instance");

    let manifest = local_manifest("Safe");
    let file = File::create(&archive_path).expect("create malicious pack");
    let mut archive = ZipWriter::new(file);
    let options = SimpleFileOptions::default();

    archive
        .start_file(ARCHIVE_MANIFEST_NAME, options)
        .expect("manifest entry");
    archive
        .write_all(&serde_json::to_vec(&manifest).expect("serialize manifest"))
        .expect("write manifest");

    archive
        .start_file("mods/Safe/../../escaped.txt", options)
        .expect("malicious entry");
    archive.write_all(b"escaped").expect("write malicious data");
    archive.finish().expect("finish malicious pack");

    import_archive(&archive_path, &instance, &mut |_| {}).expect("import malicious pack");

    assert!(!directory.join("escaped.txt").exists());
    assert!(!instance.join("escaped.txt").exists());
    assert!(!instance.join("mods").join("Safe").exists());
    assert!(instance.join(INSTANCE_MANIFEST_NAME).is_file());

    fs::remove_dir_all(directory).expect("remove test directory");
}

#[test]
fn save_files_are_discovered_from_the_saves_folder() {
    let directory = test_directory("discover-saves");
    let saves_root = directory.join("Saves");

    write_file(&saves_root.join("Colony.rws"), b"save");
    write_file(&saves_root.join("Uppercase.RWS"), b"save");
    write_file(&saves_root.join("notes.txt"), b"not a save");

    let saves = discover_save_files(&directory);

    assert_eq!(saves.len(), 2);
    assert_eq!(saves[0].0, "Colony.rws");
    assert_eq!(saves[1].0, "Uppercase.RWS");

    fs::remove_dir_all(directory).expect("remove test directory");
}

#[test]
fn tags_are_normalized_and_deduplicated() {
    assert_eq!(
        normalize_tags(" Quality of life, fixes\nFIXES,  , performance "),
        ["Quality of life", "fixes", "performance"]
    );

    let long_tag = "a".repeat(MAX_TAG_CHARS + 10);
    let tags = normalize_tags(&long_tag);

    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0].chars().count(), MAX_TAG_CHARS);
}

#[test]
fn dangerous_file_components_are_sanitized() {
    assert_eq!(sanitize_component("../Mod"), "_Mod");
    assert_eq!(sanitize_component(".."), "mod");
    assert_eq!(sanitize_component("CON"), "_CON");
    assert_eq!(sanitize_component("My:Mod?"), "My_Mod_");
    assert_eq!(sanitize_file_stem("My/Pack: Name"), "My_Pack_ Name");
}

#[test]
fn icons_are_resized_and_encoded_as_png() {
    let image = image::DynamicImage::new_rgba8(1024, 512);
    let mut encoded = Vec::new();

    image
        .write_to(
            &mut std::io::Cursor::new(&mut encoded),
            image::ImageFormat::Png,
        )
        .expect("encode source image");

    let normalized = normalize_icon(&encoded).expect("normalize icon");
    let decoded = image::load_from_memory(&normalized).expect("decode normalized icon");

    assert_eq!((decoded.width(), decoded.height()), (512, 256));
    assert!(normalized.starts_with(&[0x89, b'P', b'N', b'G']));
}

#[test]
fn missing_manifest_is_reported() {
    let directory = test_directory("missing-manifest");
    let archive_path = directory.join("empty.rimpack");
    let file = File::create(&archive_path).expect("create empty pack");
    let mut archive = ZipWriter::new(file);

    archive
        .start_file("unrelated.txt", SimpleFileOptions::default())
        .expect("write unrelated file");
    archive.write_all(b"nothing").expect("write file");
    archive.finish().expect("finish archive");

    let result = read_archive_manifest(&archive_path);

    assert!(result.is_err());

    fs::remove_dir_all(directory).expect("remove test directory");
}

#[test]
fn archive_preview_reads_manifest_and_icon_without_extracting() {
    let directory = test_directory("archive-preview");
    let instance = directory.join("instance");
    let exported = directory.join("preview.rimpack");
    let mod_folder = directory.join("source-mod");
    let icon: &[u8] = b"\x89PNG-preview-icon";
    let manifest = local_manifest("LocalMod");

    write_file(&mod_folder.join("About").join("About.xml"), b"<Mod/>");

    write_instance(
        &instance,
        PackWriteRequest {
            manifest: &manifest,
            icon: Some(icon),
            local_mods: &[("LocalMod".to_owned(), mod_folder)],
            configs: &[],
            saves: &[],
        },
        &mut |_| {},
    )
    .expect("write instance");

    export_instance(&instance, &exported, &mut |_| {}).expect("export instance");

    let preview = read_archive_preview(&exported).expect("read archive preview");
    assert_eq!(preview.manifest.name, "Test pack");
    assert_eq!(preview.manifest.mods.len(), 3);
    assert_eq!(preview.icon.as_deref(), Some(icon));

    fs::remove_dir_all(directory).expect("remove test directory");
}

#[test]
fn duplicate_instance_copies_all_contents_with_new_name() {
    let directory = test_directory("duplicate");
    let source = directory.join("source");
    let copy = directory.join("copy");
    let mod_folder = directory.join("source-mod");
    let config_root = directory.join("source-config");
    let save_path = directory.join("Colony.rws");

    write_file(&mod_folder.join("About").join("About.xml"), b"<Mod/>");
    write_file(&config_root.join("Mod_Test.xml"), b"<cfg/>");
    write_file(&save_path, b"save-bytes");

    let manifest = local_manifest("LocalMod");
    let configs = discover_config_files(&config_root);
    let saves = vec![("Colony.rws".to_owned(), save_path)];
    let icon: &[u8] = b"icon-bytes";

    write_instance(
        &source,
        PackWriteRequest {
            manifest: &manifest,
            icon: Some(icon),
            local_mods: &[("LocalMod".to_owned(), mod_folder)],
            configs: &configs,
            saves: &saves,
        },
        &mut |_| {},
    )
    .expect("write source");

    let summary = duplicate_instance(&source, &copy, "Test pack (Copy)").expect("duplicate");
    assert_eq!(summary.manifest.name, "Test pack (Copy)");
    assert_eq!(summary.manifest.mods.len(), 3);
    assert_eq!(summary.manifest.configs, ["Mod_Test.xml"]);
    assert_eq!(summary.manifest.saves, ["Colony.rws"]);
    assert_eq!(summary.icon.as_deref(), Some(icon));
    assert!(summary.size_bytes > 0);
    assert!(
        copy.join("mods")
            .join("LocalMod")
            .join("About")
            .join("About.xml")
            .is_file()
    );
    assert!(copy.join("configs").join("Mod_Test.xml").is_file());
    assert!(copy.join("saves").join("Colony.rws").is_file());

    fs::remove_dir_all(directory).expect("remove test directory");
}

#[test]
fn apply_options_can_skip_configs_and_saves() {
    let directory = test_directory("apply-options");
    let instance = directory.join("instance");
    let mod_folder = directory.join("source-mod");
    let source_config = directory.join("source-config");
    let save_path = directory.join("Colony.rws");
    let mods_root = directory.join("Mods");
    let config_root = directory.join("Config");
    let saves_root = directory.join("Saves");

    write_file(&mod_folder.join("About").join("About.xml"), b"<Mod/>");
    write_file(&source_config.join("Mod_Test.xml"), b"<cfg/>");
    write_file(&save_path, b"save-bytes");

    let manifest = local_manifest("LocalMod");
    let configs = discover_config_files(&source_config);
    let saves = vec![("Colony.rws".to_owned(), save_path)];

    write_instance(
        &instance,
        PackWriteRequest {
            manifest: &manifest,
            icon: None,
            local_mods: &[("LocalMod".to_owned(), mod_folder)],
            configs: &configs,
            saves: &saves,
        },
        &mut |_| {},
    )
    .expect("write instance");

    let applied = apply_instance_with_options(
        &instance,
        &mods_root,
        &config_root,
        &saves_root,
        PackApplyOptions {
            restore_configs: false,
            restore_saves: false,
            overwrite_saves: false,
        },
        &mut |_| {},
    )
    .expect("apply without configs/saves");

    assert_eq!(applied.installed_local_mods, ["LocalMod"]);
    assert!(applied.restored_configs.is_empty());
    assert!(applied.restored_saves.is_empty());
    assert!(
        mods_root
            .join("LocalMod")
            .join("About")
            .join("About.xml")
            .is_file()
    );
    assert!(!config_root.join("Mod_Test.xml").exists());
    assert!(!saves_root.join("Colony.rws").exists());

    fs::remove_dir_all(directory).expect("remove test directory");
}

#[test]
fn editing_instance_in_place_prunes_removed_items_and_keeps_selected_files() {
    let directory = test_directory("edit-in-place");
    let instance = directory.join("instance");
    let mod_folder = directory.join("source-mod");
    let source_config = directory.join("source-config");
    let save_a = directory.join("A.rws");
    let save_b = directory.join("B.rws");

    write_file(&mod_folder.join("About").join("About.xml"), b"<Mod/>");
    write_file(&source_config.join("Mod_Keep.xml"), b"<keep/>");
    write_file(&source_config.join("Mod_Drop.xml"), b"<drop/>");
    write_file(&save_a, b"save-a");
    write_file(&save_b, b"save-b");

    let manifest = local_manifest("LocalMod");
    let configs = discover_config_files(&source_config);
    let saves = vec![("A.rws".to_owned(), save_a), ("B.rws".to_owned(), save_b)];

    write_instance(
        &instance,
        PackWriteRequest {
            manifest: &manifest,
            icon: Some(b"icon-v1"),
            local_mods: &[("LocalMod".to_owned(), mod_folder)],
            configs: &configs,
            saves: &saves,
        },
        &mut |_| {},
    )
    .expect("write initial instance");

    // Simulate EditDetails: load existing items from the instance directory,
    // prune Mod_Drop.xml and B.rws, rename the pack, and write back to `instance`.
    let loaded = read_instance(&instance).expect("read instance");
    let mut edited_manifest = loaded.manifest.clone();
    edited_manifest.name = "Renamed Pack".to_owned();

    let kept_local_mods = instance_local_mods(&instance, &loaded.manifest);
    let kept_configs: Vec<_> = instance_configs(&instance, &loaded.manifest)
        .into_iter()
        .filter(|(rel, _)| rel == "Mod_Keep.xml")
        .collect();
    let kept_saves: Vec<_> = instance_saves(&instance, &loaded.manifest)
        .into_iter()
        .filter(|(name, _)| name == "A.rws")
        .collect();

    write_instance(
        &instance,
        PackWriteRequest {
            manifest: &edited_manifest,
            icon: loaded.icon.as_deref(),
            local_mods: &kept_local_mods,
            configs: &kept_configs,
            saves: &kept_saves,
        },
        &mut |_| {},
    )
    .expect("write edited instance in place");

    let reloaded = read_instance(&instance).expect("reload edited instance");
    assert_eq!(reloaded.manifest.name, "Renamed Pack");
    assert_eq!(reloaded.manifest.configs, ["Mod_Keep.xml"]);
    assert_eq!(reloaded.manifest.saves, ["A.rws"]);
    assert_eq!(reloaded.icon.as_deref(), Some(b"icon-v1".as_slice()));
    assert!(
        instance
            .join("mods")
            .join("LocalMod")
            .join("About")
            .join("About.xml")
            .is_file()
    );
    assert!(instance.join("configs").join("Mod_Keep.xml").is_file());
    assert!(!instance.join("configs").join("Mod_Drop.xml").exists());
    assert!(instance.join("saves").join("A.rws").is_file());
    assert!(!instance.join("saves").join("B.rws").exists());

    fs::remove_dir_all(directory).expect("remove test directory");
}
