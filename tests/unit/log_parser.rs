use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use super::*;

static NEXT_TEST_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TestDirectory {
    path: PathBuf,
}

impl TestDirectory {
    fn new() -> io::Result<Self> {
        let unique_number = NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "rimmod-log-parser-test-{}-{unique_number}",
            std::process::id()
        ));

        fs::create_dir(&path)?;

        Ok(Self { path })
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

const STRUCTURED_LOG: &str = concat!(
    "=== Waylith log ===\n",
    "Started:   2026-11-22 07:51:02.589 +03:00\n",
    "Version:   1.7.5014.1195\n",
    "OS:        Microsoft Windows 10.0.26300 (X64)\n",
    "Runtime:   .NET 10.0.15-waylithclr.dev\n",
    "Process:   pid 20548\n",
    "Log file:  C:\\Users\\test\\AppData\\LocalLow\\Ludeon Studios\\RimWorld by Ludeon Studios\\Logs\\Player.log\n",
    "Command:   RimWorld.dll -mod=mlie.noversionwarning\n",
    "\n",
    "2026-11-22 07:51:03.273  INFO [VulkanStartup] Vulkan loader ready.\n",
    "    at Waylith.Graphics.VulkanStartup.SelectWindowsLoader(String bundledPath) [0x334]\n",
    "\n",
    "2026-11-22 07:51:03.527  WARN [Vulkan] Loader message.\n",
    "Objects: 1\n",
    "    [0] VkInstance 0x1dcaec26360\n",
    "    at Brutal.VulkanApi.Instance..ctor(CreateInfo createInfo) [0x2DA]\n",
    "\n",
    "2026-11-22 07:51:04.000  ERROR [Verse] XML error: Foo doesn't correspond to any field in type ThingDef. Context: BrokenDef test value\n",
    "    at Verse.XmlToObjectUtils.DoFieldSearch(Type typeBeingDeserialized, XmlNode fieldNode, XmlNode xmlRootForDebug) [0x227]\n",
    "    at TestMod.Core.Patch.Method() [0x1]\n",
    "\n",
    "2026-11-22 07:51:05.000  DEBUG [Verbose] Verbose logging enabled.\n",
);

#[test]
fn parses_waylith_header_and_levels() {
    let document = parse_log_text(STRUCTURED_LOG);

    assert_eq!(document.format, LogFormat::Structured);
    assert_eq!(document.entries.len(), 4);
    assert_eq!(
        document.header.started.as_deref(),
        Some("2026-11-22 07:51:02.589 +03:00")
    );
    assert_eq!(document.header.version.as_deref(), Some("1.7.5014.1195"));
    assert_eq!(
        document.header.runtime.as_deref(),
        Some(".NET 10.0.15-waylithclr.dev")
    );

    let first = &document.entries[0];
    assert_eq!(first.level, LogLevel::Info);
    assert_eq!(first.category.as_deref(), Some("VulkanStartup"));
    assert_eq!(first.message, "Vulkan loader ready.");
    assert_eq!(first.line_number, 10);
    assert_eq!(first.details.len(), 1);
    assert!(first.details[0].contains("SelectWindowsLoader"));

    let warning = &document.entries[1];
    assert_eq!(warning.level, LogLevel::Warning);
    assert_eq!(warning.details.len(), 3);
    assert!(
        warning
            .details
            .iter()
            .any(|line| line.contains("Objects: 1"))
    );

    let error = &document.entries[2];
    assert_eq!(error.level, LogLevel::Error);
    assert!(error.message.starts_with("XML error:"));
    assert_eq!(error.details.len(), 2);

    assert_eq!(document.entries[3].level, LogLevel::Debug);
}

#[test]
fn keeps_multiline_mod_messages_attached() {
    let text = concat!(
        "2026-11-13 22:07:22.528  ERROR [Verse] Mod NewRatkinPlus_zh did not load any content. Following load folders were used:\n",
        " - D:\\RimWorld\\Mods\\updated-1792199727\\Cont\n",
        " - D:\\RimWorld\\Mods\\updated-1792199727\n",
        "    at Verse.LoadedModManager.LoadAllActiveMods() [0x132]\n",
    );

    let document = parse_log_text(text);

    assert_eq!(document.entries.len(), 1);
    assert!(
        document.entries[0]
            .message
            .contains("Following load folders")
    );
    assert_eq!(document.entries[0].details.len(), 3);
}

const LEGACY_LOG: &str = concat!(
    "Mono path[0] = 'C:/RimWorld/RimWorldWin64_Data/Managed'\n",
    "Initialize engine version: 2022.3.44f1\n",
    "RimWorld 1.6.4518 rev84\n",
    "[HugsLib] version 12.0.0\n",
    "(Filename: C:\\buildslave\\unity\\build\\Runtime/Export/Debug/Debug.bindings.h Line: 35)\n",
    "XML error: Could not find parent node named \"BenchBase\" for node \"ThingDef\". Full node: FM_BasicManagerStation manager desk\n",
    "(Filename: C:\\buildslave\\unity\\build\\Runtime/Export/Debug/Debug.bindings.h Line: 39)\n",
    " at Verse.XmlInheritance.TryRegister(XmlNode node, ModContentPack mod)\n",
    " at Verse.LoadedModManager.LoadAllActiveMods ()\n",
    "Could not resolve cross-reference: No SoundDef named Slurp found to give to ThingDef SomeName.\n",
);

#[test]
fn parses_legacy_unity_log() {
    let document = parse_log_text(LEGACY_LOG);

    assert_eq!(document.format, LogFormat::Legacy);
    assert_eq!(
        document.header.version.as_deref(),
        Some("RimWorld 1.6.4518 rev84")
    );
    assert_eq!(document.entries.len(), 4);

    assert_eq!(document.entries[0].message, "RimWorld 1.6.4518 rev84");
    assert_eq!(document.entries[0].level, LogLevel::Info);

    assert_eq!(document.entries[1].message, "[HugsLib] version 12.0.0");
    assert_eq!(document.entries[1].level, LogLevel::Info);

    let error = &document.entries[2];
    assert_eq!(error.level, LogLevel::Error);
    assert!(error.message.starts_with("XML error:"));
    assert!(error.details.iter().any(|line| line.contains("Filename:")));
    assert!(
        error
            .details
            .iter()
            .any(|line| line.contains("TryRegister"))
    );

    let reference_error = &document.entries[3];
    assert_eq!(reference_error.level, LogLevel::Error);
    assert!(
        reference_error
            .message
            .starts_with("Could not resolve cross-reference")
    );
}

#[test]
fn discovers_current_and_previous_logs_in_both_layouts() -> io::Result<()> {
    let test_directory = TestDirectory::new()?;
    let config = test_directory.path.join("Config");
    let logs = test_directory.path.join("Logs");

    fs::create_dir_all(&config)?;
    fs::create_dir_all(&logs)?;
    fs::write(logs.join("Player.log"), STRUCTURED_LOG)?;
    fs::write(
        logs.join("Player-prev.log"),
        "2026-11-20 10:00:00.000  INFO [Verse] previous\n",
    )?;
    fs::write(
        test_directory.path.join("Player.log"),
        "stale 1.6 log that must not win over Logs/Player.log\n",
    )?;

    let files = discover_log_files(&config);

    assert_eq!(files.len(), 2);
    assert_eq!(files[0].session, LogSession::Current);
    assert_eq!(files[0].path, logs.join("Player.log"));
    assert_eq!(files[1].session, LogSession::Previous);
    assert_eq!(files[1].path, logs.join("Player-prev.log"));

    Ok(())
}

#[test]
fn falls_back_to_root_player_log() -> io::Result<()> {
    let test_directory = TestDirectory::new()?;
    let config = test_directory.path.join("Config");

    fs::create_dir_all(&config)?;
    fs::write(test_directory.path.join("Player.log"), LEGACY_LOG)?;

    let files = discover_log_files(&config);

    assert_eq!(files.len(), 1);
    assert_eq!(files[0].session, LogSession::Current);
    assert_eq!(files[0].path, test_directory.path.join("Player.log"));

    Ok(())
}
