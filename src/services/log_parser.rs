//! Reading and parsing RimWorld's `Player.log`.
//!
//! RimWorld 1.7 (Waylith) writes structured lines with a timestamp, level and
//! category to `Logs/Player.log`. RimWorld 1.6 and older write Unity's plain
//! text log to `Player.log` next to the config folder. Both layouts end up in
//! the same in-memory shape so the rest of the app does not care which game
//! version produced the file.

use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Logs above this size are read in two pieces. RimWorld can produce enormous
/// logs when an error repeats every tick, and loading all of that into memory
/// only makes the log viewer slower.
const MAX_LOG_BYTES: u64 = 64 * 1024 * 1024;

/// Kept from the beginning so startup and mod-loading errors survive on huge
/// logs. The remainder of the budget is read from the end of the file.
const HEAD_PORTION_BYTES: u64 = 8 * 1024 * 1024;

const TRUNCATION_CATEGORY: &str = "RimMod";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum LogLevel {
    Debug,
    Info,
    Warning,
    Error,
}

impl LogLevel {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Debug => "Debug",
            Self::Info => "Info",
            Self::Warning => "Warning",
            Self::Error => "Error",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LogFormat {
    /// RimWorld 1.7 / Waylith: timestamped, levelled lines.
    Structured,

    /// RimWorld 1.6 and older: Unity's Player.log text format.
    Legacy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum LogSession {
    Current,
    Previous,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct LogHeader {
    pub(crate) started: Option<String>,
    pub(crate) version: Option<String>,
    pub(crate) os: Option<String>,
    pub(crate) runtime: Option<String>,
    pub(crate) command: Option<String>,
    pub(crate) documented_path: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct LogEntry {
    pub(crate) level: LogLevel,
    pub(crate) timestamp: Option<String>,
    pub(crate) category: Option<String>,
    pub(crate) message: String,

    /// Stack frames, exception text, `[Ref ...]` markers and other lines that
    /// belong to the message above.
    pub(crate) details: Vec<String>,

    /// One-based line number in the original file. Zero when the entry came
    /// from the tail of a log that was too large to read in one piece.
    pub(crate) line_number: usize,
}

impl LogEntry {
    pub(crate) fn full_text(&self) -> String {
        let mut text = self.message.clone();

        for detail in &self.details {
            text.push('\n');
            text.push_str(detail);
        }

        text
    }
}

#[derive(Debug, Clone)]
pub(crate) struct LogDocument {
    pub(crate) format: LogFormat,
    pub(crate) header: LogHeader,
    pub(crate) entries: Vec<LogEntry>,
    pub(crate) truncated: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct LogFileInfo {
    pub(crate) session: LogSession,
    pub(crate) path: PathBuf,
    pub(crate) size: u64,
}

/// Find the current and previous session logs next to the `Config` folder.
///
/// RimWorld 1.7 keeps the files in a `Logs` subfolder; older versions put them
/// beside `Config`. The `Logs` location wins when both exist so a stale 1.6 log
/// left at the root never hides the current 1.7 one.
pub(crate) fn discover_log_files(config_path: &Path) -> Vec<LogFileInfo> {
    let Some(data_directory) = config_path.parent() else {
        return Vec::new();
    };

    let mut files = Vec::new();

    for (session, candidates) in [
        (
            LogSession::Current,
            [
                data_directory.join("Logs").join("Player.log"),
                data_directory.join("Player.log"),
            ],
        ),
        (
            LogSession::Previous,
            [
                data_directory.join("Logs").join("Player-prev.log"),
                data_directory.join("Player-prev.log"),
            ],
        ),
    ] {
        let Some((path, metadata)) = candidates.into_iter().find_map(|path| {
            let metadata = fs::metadata(&path).ok()?;

            metadata.is_file().then_some((path, metadata))
        }) else {
            continue;
        };

        files.push(LogFileInfo {
            session,
            path,
            size: metadata.len(),
        });
    }

    files
}

pub(crate) fn parse_log_file(path: &Path) -> io::Result<LogDocument> {
    let metadata = fs::metadata(path)?;

    if metadata.len() <= MAX_LOG_BYTES {
        let bytes = fs::read(path)?;

        return Ok(parse_log_text(&String::from_utf8_lossy(&bytes)));
    }

    let head = read_range(path, 0, HEAD_PORTION_BYTES)?;
    let head_text = keep_complete_lines(&head);
    let mut document = parse_log_text(&head_text);
    document.truncated = true;

    let tail_length = MAX_LOG_BYTES - HEAD_PORTION_BYTES;
    let tail_start = metadata.len().saturating_sub(tail_length);
    let tail = read_range(path, tail_start, tail_length)?;
    let tail_text = skip_partial_line(&tail);
    let tail_document = parse_log_text_as(&tail_text, document.format, false);

    document.entries.push(LogEntry {
        level: LogLevel::Info,
        timestamp: None,
        category: Some(TRUNCATION_CATEGORY.to_owned()),
        message: format!(
            "Log is {:.1} MB; showing the first {:.0} MB and the last {:.0} MB. \
             Open the file to see everything.",
            metadata.len() as f64 / (1024.0 * 1024.0),
            HEAD_PORTION_BYTES as f64 / (1024.0 * 1024.0),
            tail_length as f64 / (1024.0 * 1024.0),
        ),
        details: Vec::new(),
        line_number: 0,
    });

    document.entries.extend(tail_document.entries);

    Ok(document)
}

fn read_range(path: &Path, start: u64, length: u64) -> io::Result<Vec<u8>> {
    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(start))?;

    let mut bytes = Vec::new();
    file.take(length).read_to_end(&mut bytes)?;

    Ok(bytes)
}

fn keep_complete_lines(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);

    match text.rfind('\n') {
        Some(last_newline) => text[..=last_newline].to_owned(),
        None => String::new(),
    }
}

fn skip_partial_line(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);

    match text.find('\n') {
        Some(first_newline) => text[first_newline + 1..].to_owned(),
        None => String::new(),
    }
}

pub(crate) fn parse_log_text(text: &str) -> LogDocument {
    let format = detect_format(text);

    parse_log_text_as(text, format, true)
}

fn parse_log_text_as(text: &str, format: LogFormat, collect_header: bool) -> LogDocument {
    match format {
        LogFormat::Structured => parse_structured(text, collect_header),
        LogFormat::Legacy => parse_legacy(text, collect_header),
    }
}

fn detect_format(text: &str) -> LogFormat {
    for line in text.lines().take(200) {
        if structured_prefix(line).is_some() {
            return LogFormat::Structured;
        }

        if line.trim_start().starts_with("=== Waylith log ===") {
            return LogFormat::Structured;
        }
    }

    LogFormat::Legacy
}

fn parse_structured(text: &str, collect_header: bool) -> LogDocument {
    let mut document = LogDocument {
        format: LogFormat::Structured,
        header: LogHeader::default(),
        entries: Vec::new(),
        truncated: false,
    };

    let mut current: Option<LogEntry> = None;

    for (index, raw_line) in text.lines().enumerate() {
        let line = raw_line
            .trim_end_matches('\r')
            .trim_start_matches('\u{feff}');

        if let Some((timestamp, level, category, message)) = structured_prefix(line) {
            if let Some(entry) = current.take() {
                document.entries.push(entry);
            }

            current = Some(LogEntry {
                level,
                timestamp: Some(timestamp),
                category,
                message,
                details: Vec::new(),
                line_number: index + 1,
            });

            continue;
        }

        if let Some(entry) = current.as_mut() {
            if !line.is_empty() {
                entry.details.push(line.to_owned());
            }
        } else if collect_header {
            update_header(&mut document.header, line);
        }
    }

    if let Some(entry) = current {
        document.entries.push(entry);
    }

    document
}

fn structured_prefix(line: &str) -> Option<(String, LogLevel, Option<String>, String)> {
    let bytes = line.as_bytes();

    if bytes.len() < 24 || !is_timestamp(&bytes[..23]) {
        return None;
    }

    let timestamp = line[..23].to_owned();
    let rest = line[23..].trim_start();
    let mut parts = rest.splitn(2, char::is_whitespace);
    let level = parse_level(parts.next()?)?;
    let rest = parts.next().unwrap_or_default().trim_start();

    let (category, message) = match rest.strip_prefix('[') {
        Some(after_bracket) => {
            let end = after_bracket.find(']')?;

            (
                Some(after_bracket[..end].to_owned()),
                after_bracket[end + 1..].trim_start().to_owned(),
            )
        }

        None => (None, rest.to_owned()),
    };

    Some((timestamp, level, category, message))
}

fn is_timestamp(bytes: &[u8]) -> bool {
    bytes.len() == 23
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b' '
        && bytes[13] == b':'
        && bytes[16] == b':'
        && bytes[19] == b'.'
        && bytes.iter().enumerate().all(|(index, byte)| {
            matches!(index, 4 | 7 | 10 | 13 | 16 | 19) || byte.is_ascii_digit()
        })
}

fn parse_level(token: &str) -> Option<LogLevel> {
    match token {
        "ERROR" | "FATAL" => Some(LogLevel::Error),
        "WARN" | "WARNING" => Some(LogLevel::Warning),
        "INFO" => Some(LogLevel::Info),
        "DEBUG" | "TRACE" => Some(LogLevel::Debug),
        _ => None,
    }
}

fn update_header(header: &mut LogHeader, line: &str) {
    let Some((key, value)) = line.split_once(':') else {
        return;
    };

    let value = value.trim();

    if value.is_empty() {
        return;
    }

    match key.trim() {
        "Started" => header.started = Some(value.to_owned()),
        "Version" => header.version = Some(value.to_owned()),
        "OS" => header.os = Some(value.to_owned()),
        "Runtime" => header.runtime = Some(value.to_owned()),
        "Command" => header.command = Some(value.to_owned()),
        "Log file" => header.documented_path = Some(value.to_owned()),
        _ => {}
    }
}

fn parse_legacy(text: &str, collect_header: bool) -> LogDocument {
    let mut document = LogDocument {
        format: LogFormat::Legacy,
        header: LogHeader::default(),
        entries: Vec::new(),
        truncated: false,
    };

    let mut current: Option<LogEntry> = None;
    let mut entries_started = !collect_header;

    for (index, raw_line) in text.lines().enumerate() {
        let line = raw_line
            .trim_end_matches('\r')
            .trim_start_matches('\u{feff}');
        let trimmed = line.trim();

        if trimmed.is_empty() {
            continue;
        }

        if !entries_started {
            if trimmed.starts_with("RimWorld ") {
                document.header.version = Some(trimmed.to_owned());
                entries_started = true;
            } else if is_engine_header_line(trimmed) {
                continue;
            } else {
                entries_started = true;
            }
        }

        if is_legacy_continuation(line, trimmed) {
            if let Some(entry) = current.as_mut() {
                entry.details.push(trimmed.to_owned());
            }

            continue;
        }

        if let Some(entry) = current.take() {
            document.entries.push(entry);
        }

        current = Some(LogEntry {
            level: infer_legacy_level(trimmed),
            timestamp: None,
            category: None,
            message: trimmed.to_owned(),
            details: Vec::new(),
            line_number: index + 1,
        });
    }

    if let Some(entry) = current {
        document.entries.push(entry);
    }

    document
}

fn is_engine_header_line(line: &str) -> bool {
    const PREFIXES: &[&str] = &[
        "Mono path[",
        "Mono config path",
        "Initialize engine version",
        "Version:",
        "GfxDevice:",
        "Renderer:",
        "Vendor:",
        "VRAM:",
        "Begin MonoManager ReloadAssembly",
        "Loading player data",
        "UnloadTime:",
        "Unloading ",
        "PlayerConnection",
        "Crash!!!",
        "[Subsystems]",
        "[Physics]",
        "[Audio]",
        "[FMOD]",
        "WARNING: Shader Unsupported",
        "Shader '",
        "Platform:",
        "Command-line:",
        "SystemInfo",
    ];

    PREFIXES.iter().any(|prefix| line.starts_with(prefix))
}

fn is_legacy_continuation(line: &str, trimmed: &str) -> bool {
    line.starts_with(char::is_whitespace)
        || line.starts_with('\t')
        || trimmed.starts_with("at ")
        || trimmed.starts_with("---")
        || trimmed.starts_with("[Ref ")
        || trimmed.starts_with("(Filename:")
        || trimmed.starts_with(" - ")
        || trimmed.starts_with("Objects:")
        || trimmed.starts_with('[')
            && trimmed.split_once(']').is_some_and(|(_, rest)| {
                rest.trim_start()
                    .starts_with(|c: char| c.is_ascii_hexdigit())
            })
        || looks_like_legacy_frame(trimmed)
}

fn looks_like_legacy_frame(line: &str) -> bool {
    let Some((type_name, rest)) = line.split_once(':') else {
        return false;
    };

    if type_name.is_empty()
        || type_name.starts_with('[')
        || type_name.contains(char::is_whitespace)
        || !type_name.contains('.')
    {
        return false;
    }

    let method = rest.split([' ', '(']).next().unwrap_or_default();

    !method.is_empty()
        && method
            .chars()
            .all(|character| character.is_alphanumeric() || matches!(character, '_' | '<' | '>'))
}

fn infer_legacy_level(message: &str) -> LogLevel {
    let upper = message.to_ascii_uppercase();

    if upper.contains("[ERR]")
        || upper.contains("[ERROR]")
        || upper.starts_with("ERROR")
        || upper.starts_with("XML ERROR")
        || upper.contains("EXCEPTION")
        || upper.starts_with("COULD NOT")
        || upper.starts_with("FAILED")
        || upper.starts_with("CAUGHT EXCEPTION")
        || upper.starts_with("UNABLE TO")
        || upper.starts_with("CANNOT")
        || upper.starts_with("[REF ")
    {
        return LogLevel::Error;
    }

    if upper.contains("[WRN]")
        || upper.contains("[WARN]")
        || upper.starts_with("WARNING")
        || upper.starts_with("WARN")
    {
        return LogLevel::Warning;
    }

    if upper.contains("[DBG]") || upper.contains("[DEBUG]") || upper.starts_with("DEBUG") {
        return LogLevel::Debug;
    }

    LogLevel::Info
}

#[cfg(test)]
#[path = "../../tests/unit/log_parser.rs"]
mod tests;
