//! Turn a parsed log into a small list of issues and work out which installed
//! mods each issue is most likely to come from.
//!
//! Attribution is deliberately evidence based. A single error can have more
//! than one plausible owner, and load-order problems often have no owner at
//! all. The UI shows every suspect together with the evidence that matched.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::models::{ModCollection, ModId, ModSource, PackageId};
use crate::services::log_parser::{LogDocument, LogEntry, LogLevel};

const MAX_DLL_BYTES: u64 = 128 * 1024 * 1024;
const MAX_DEF_BYTES: u64 = 512 * 1024 * 1024;
const MAX_XML_FILE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_EVIDENCE: usize = 4;
const MAX_SUSPECTS_PER_ISSUE: usize = 8;

const GENERIC_MOD_NAMES: &[&str] = &[
    "core",
    "harmony",
    "hugs lib",
    "hugslib",
    "framework",
    "library",
    "mod",
    "mods",
    "unofficial updates",
    "vanilla",
];

/// Shared tooling that is almost never the cause of a modlist failure. It can
/// still appear on an individual issue, but it should not take over the
/// “Most suspected” ranking.
const NON_CULPRIT_PACKAGE_IDS: &[&str] = &[
    "brrainz.harmony",
    "unlimitedhugs.hugslib",
    "mlie.noversionwarning",
    "ilyvion.loadingprogress",
];

const DEF_STOPWORDS: &[&str] = &[
    "true",
    "false",
    "normal",
    "none",
    "item",
    "null",
    "with",
    "from",
    "this",
    "that",
    "thing",
    "def",
    "defname",
    "context",
    "full",
    "node",
    "class",
    "type",
    "value",
    "values",
    "default",
    "graphic",
    "multi",
    "cutout",
    "complex",
    "stuff",
    "stuffcategories",
    "label",
    "description",
    "building",
    "pawn",
    "human",
    "weapon",
    "apparel",
];

#[derive(Debug, Clone)]
pub(crate) struct ModSnapshot {
    pub(crate) package_id: PackageId,
    pub(crate) name: String,
    pub(crate) folder: PathBuf,
    pub(crate) workshop_id: Option<u64>,
    pub(crate) enabled: bool,
    pub(crate) official: bool,

    assembly_paths: Vec<PathBuf>,
    assembly_stems: Vec<String>,
}

impl ModSnapshot {
    pub(crate) fn collect(mods: &ModCollection) -> Vec<Self> {
        mods.all
            .iter()
            .enumerate()
            .map(|(index, rimworld_mod)| {
                let workshop_id = match &rimworld_mod.source {
                    ModSource::SteamWorkshop { workshop_id } => Some(*workshop_id),
                    _ => None,
                };

                let mut assembly_paths = Vec::new();
                let mut assembly_stems = Vec::new();
                collect_assemblies(
                    &rimworld_mod.folder.join("Assemblies"),
                    &mut assembly_paths,
                    &mut assembly_stems,
                );
                assembly_stems.sort();
                assembly_stems.dedup();

                Self {
                    package_id: rimworld_mod.package_id.clone(),
                    name: rimworld_mod.name.clone(),
                    folder: rimworld_mod.folder.clone(),
                    workshop_id,
                    enabled: mods.enabled_ids().contains(&ModId::from_index(index)),
                    official: matches!(rimworld_mod.source, ModSource::Official),
                    assembly_paths,
                    assembly_stems,
                }
            })
            .collect()
    }
}

fn collect_assemblies(folder: &Path, paths: &mut Vec<PathBuf>, stems: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };

        if file_type.is_dir() {
            collect_assemblies(&path, paths, stems);
        } else if file_type.is_file()
            && path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("dll"))
        {
            let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };

            let normalized = normalize_assembly_stem(stem);

            if normalized.len() >= 4 {
                stems.push(normalized);
            }

            paths.push(path);
        }
    }
}

fn normalize_assembly_stem(stem: &str) -> String {
    stem.trim_start_matches(|character: char| {
        character.is_ascii_digit() || matches!(character, '_' | '-' | '.')
    })
    .to_ascii_lowercase()
}

#[derive(Debug, Clone)]
pub(crate) struct ModSuspect {
    pub(crate) package_id: PackageId,
    pub(crate) name: String,
    pub(crate) enabled: bool,
    pub(crate) official: bool,
    pub(crate) score: u32,
    pub(crate) evidence: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct LogIssue {
    pub(crate) level: LogLevel,
    pub(crate) title: String,
    pub(crate) category: Option<String>,
    pub(crate) count: usize,
    pub(crate) first_index: usize,
    pub(crate) last_index: usize,
    pub(crate) samples: Vec<usize>,
    pub(crate) suspects: Vec<ModSuspect>,
}

#[derive(Debug, Clone)]
pub(crate) struct SuspectSummary {
    pub(crate) package_id: PackageId,
    pub(crate) name: String,
    pub(crate) enabled: bool,
    pub(crate) errors: usize,
    pub(crate) warnings: usize,
    pub(crate) issues: usize,
    pub(crate) score: u32,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct LevelCounts {
    pub(crate) error: usize,
    pub(crate) warning: usize,
    pub(crate) info: usize,
    pub(crate) debug: usize,
}

impl LevelCounts {
    pub(crate) fn total(self) -> usize {
        self.error + self.warning + self.info + self.debug
    }

    fn add(&mut self, level: LogLevel) {
        match level {
            LogLevel::Error => self.error += 1,
            LogLevel::Warning => self.warning += 1,
            LogLevel::Info => self.info += 1,
            LogLevel::Debug => self.debug += 1,
        }
    }
}

pub(crate) struct LogReport {
    pub(crate) document: LogDocument,
    pub(crate) issues: Vec<LogIssue>,
    pub(crate) suspects: Vec<SuspectSummary>,
    pub(crate) counts: LevelCounts,
}

pub(crate) fn analyze(document: LogDocument, mods: &[ModSnapshot]) -> LogReport {
    let mut counts = LevelCounts::default();

    for entry in &document.entries {
        counts.add(entry.level);
    }

    let mut issues = group_entries(&document.entries);
    let mut builders: Vec<HashMap<usize, SuspectBuilder>> =
        (0..issues.len()).map(|_| HashMap::new()).collect();

    for (issue_index, issue) in issues.iter().enumerate() {
        let entry = &document.entries[issue.first_index];
        attribute_directly(entry, mods, &mut builders[issue_index]);
    }

    let mut code_candidates = Vec::new();
    let mut issue_candidates: Vec<Vec<String>> = vec![Vec::new(); issues.len()];

    for (issue_index, issue) in issues.iter().enumerate() {
        if best_score(&builders[issue_index]) >= 110 {
            continue;
        }

        let entry = &document.entries[issue.first_index];
        let candidates = type_candidates(entry);

        if !candidates.is_empty() {
            code_candidates.extend(candidates.iter().cloned());
            issue_candidates[issue_index] = candidates;
        }
    }

    code_candidates.sort();
    code_candidates.dedup();

    if !code_candidates.is_empty() {
        let index = CodeIndex::build(mods, &code_candidates);

        for (issue_index, candidates) in issue_candidates.iter().enumerate() {
            if candidates.is_empty() {
                continue;
            }

            index.attribute(candidates, mods, &mut builders[issue_index]);
        }
    }

    let mut def_candidates = Vec::new();
    let mut issue_def_candidates: Vec<Vec<String>> = vec![Vec::new(); issues.len()];

    for (issue_index, issue) in issues.iter().enumerate() {
        if best_score(&builders[issue_index]) >= 70 {
            continue;
        }

        let entry = &document.entries[issue.first_index];
        let candidates = def_candidates_from(entry);

        if !candidates.is_empty() {
            def_candidates.extend(candidates.iter().cloned());
            issue_def_candidates[issue_index] = candidates;
        }
    }

    def_candidates.sort();
    def_candidates.dedup();

    if !def_candidates.is_empty() {
        let index = DefIndex::build(mods, &def_candidates);

        for (issue_index, candidates) in issue_def_candidates.iter().enumerate() {
            if candidates.is_empty() {
                continue;
            }

            index.attribute(candidates, &mut builders[issue_index]);
        }
    }

    for (issue_index, issue) in issues.iter_mut().enumerate() {
        issue.suspects = finalize_suspects(std::mem::take(&mut builders[issue_index]), mods);
    }

    let suspects = summarize_suspects(&issues, mods);

    LogReport {
        document,
        issues,
        suspects,
        counts,
    }
}

#[derive(Default)]
struct SuspectBuilder {
    score: u32,
    evidence: Vec<String>,
}

fn best_score(builders: &HashMap<usize, SuspectBuilder>) -> u32 {
    builders
        .values()
        .map(|builder| builder.score)
        .max()
        .unwrap_or(0)
}

fn push_suspect(
    builders: &mut HashMap<usize, SuspectBuilder>,
    mod_index: usize,
    score: u32,
    evidence: impl Into<String>,
) {
    let builder = builders.entry(mod_index).or_default();

    builder.score = builder.score.max(score);

    let evidence = evidence.into();

    if !builder.evidence.contains(&evidence) && builder.evidence.len() < MAX_EVIDENCE {
        builder.evidence.push(evidence);
    }
}

fn finalize_suspects(
    builders: HashMap<usize, SuspectBuilder>,
    mods: &[ModSnapshot],
) -> Vec<ModSuspect> {
    let mut suspects = builders
        .into_iter()
        .filter_map(|(mod_index, builder)| {
            let rimworld_mod = mods.get(mod_index)?;

            Some(ModSuspect {
                package_id: rimworld_mod.package_id.clone(),
                name: rimworld_mod.name.clone(),
                enabled: rimworld_mod.enabled,
                official: rimworld_mod.official,
                score: builder.score,
                evidence: builder.evidence,
            })
        })
        .collect::<Vec<_>>();

    suspects.sort_by(|left, right| {
        is_non_culprit_suspect(left)
            .cmp(&is_non_culprit_suspect(right))
            .then_with(|| right.score.cmp(&left.score))
            .then_with(|| left.name.cmp(&right.name))
    });
    suspects.truncate(MAX_SUSPECTS_PER_ISSUE);

    suspects
}

fn attribute_directly(
    entry: &LogEntry,
    mods: &[ModSnapshot],
    builders: &mut HashMap<usize, SuspectBuilder>,
) {
    let text = entry.full_text().to_lowercase();
    let path_text = text.replace('/', "\\");

    for (mod_index, rimworld_mod) in mods.iter().enumerate() {
        let package_id = rimworld_mod.package_id.as_str();

        if contains_package_id(&text, package_id) {
            push_suspect(
                builders,
                mod_index,
                120,
                format!("Package ID `{package_id}` appears in the log entry"),
            );
        }

        let folder = rimworld_mod
            .folder
            .to_string_lossy()
            .replace('/', "\\")
            .to_lowercase();

        if folder.len() >= 5 && path_text.contains(&folder) {
            push_suspect(
                builders,
                mod_index,
                120,
                "The mod's folder path appears in the log entry",
            );
        }

        if let Some(workshop_id) = rimworld_mod.workshop_id {
            let needle = format!("294100\\{workshop_id}");

            if path_text.contains(&needle) {
                push_suspect(
                    builders,
                    mod_index,
                    120,
                    format!("Workshop item {workshop_id} appears in the log entry"),
                );
            }
        }

        let name = rimworld_mod.name.to_lowercase();

        if is_distinct_mod_name(&name) && contains_delimited(&text, &name, char::is_alphanumeric) {
            push_suspect(
                builders,
                mod_index,
                75,
                format!(
                    "Mod name \"{}\" appears in the log entry",
                    rimworld_mod.name
                ),
            );
        }

        for stem in &rimworld_mod.assembly_stems {
            if stem.len() >= 4 && contains_assembly_reference(&text, stem) {
                push_suspect(
                    builders,
                    mod_index,
                    80,
                    format!("Assembly name `{stem}` appears in the log entry"),
                );
                break;
            }
        }
    }
}

fn is_distinct_mod_name(name: &str) -> bool {
    name.chars()
        .filter(|character| character.is_alphanumeric())
        .count()
        >= 5
        && !GENERIC_MOD_NAMES.contains(&name)
}

/// Match a complete identifier. A package ID must not be a prefix of a longer
/// one, so `ludeon.rimworld` does not match `ludeon.rimworld.biotech`. A dot is
/// allowed as trailing punctuation but not when another ID segment follows.
fn contains_package_id(text: &str, package_id: &str) -> bool {
    if package_id.is_empty() {
        return false;
    }

    let mut search_from = 0;

    while let Some(relative) = text[search_from..].find(package_id) {
        let start = search_from + relative;
        let end = start + package_id.len();
        let before_ok = text[..start]
            .chars()
            .next_back()
            .is_none_or(|character| !is_package_id_character(character));
        let after_text = &text[end..];
        let after_ok = match after_text.chars().next() {
            None => true,
            Some('.') => !after_text[1..]
                .chars()
                .next()
                .is_some_and(is_package_id_character),
            Some(character) => !is_package_id_character(character),
        };

        if before_ok && after_ok {
            return true;
        }

        search_from = end.max(start + 1);
    }

    false
}

fn is_package_id_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.')
}

fn contains_delimited(
    text: &str,
    needle: &str,
    is_word_character: impl Fn(char) -> bool + Copy,
) -> bool {
    if needle.is_empty() {
        return false;
    }

    let mut search_from = 0;

    while let Some(relative) = text[search_from..].find(needle) {
        let start = search_from + relative;
        let end = start + needle.len();
        let before_ok = text[..start]
            .chars()
            .next_back()
            .is_none_or(|character| !is_word_character(character));
        let after_ok = text[end..]
            .chars()
            .next()
            .is_none_or(|character| !is_word_character(character));

        if before_ok && after_ok {
            return true;
        }

        search_from = end.max(start + 1);
    }

    false
}

/// Assembly names are only evidence when the log explicitly references the
/// assembly. Plain words such as "Harmony" appear in normal mod messages and
/// must not blame the Harmony mod.
fn contains_assembly_reference(text: &str, stem: &str) -> bool {
    text.contains(&format!("assembly '{stem}'"))
        || text.contains(&format!("assembly '{stem},"))
        || text.contains(&format!("assembly \"{stem}\""))
        || text.contains(&format!("assembly \"{stem},"))
        || text.contains(&format!("{stem}.dll"))
}

fn group_entries(entries: &[LogEntry]) -> Vec<LogIssue> {
    let mut issues = Vec::<LogIssue>::new();
    let mut positions = HashMap::<(LogLevel, String, String), usize>::new();

    for (entry_index, entry) in entries.iter().enumerate() {
        let fingerprint = (
            entry.level,
            normalize_title(&entry.message),
            top_frame(entry),
        );

        if let Some(&position) = positions.get(&fingerprint) {
            let issue = &mut issues[position];
            issue.count += 1;
            issue.last_index = entry_index;
            issue.title = entry.message.clone();

            if issue.samples.len() < 32 {
                issue.samples.push(entry_index);
            }

            continue;
        }

        positions.insert(fingerprint, issues.len());
        issues.push(LogIssue {
            level: entry.level,
            title: entry.message.clone(),
            category: entry.category.clone(),
            count: 1,
            first_index: entry_index,
            last_index: entry_index,
            samples: vec![entry_index],
            suspects: Vec::new(),
        });
    }

    issues
}

fn normalize_title(title: &str) -> String {
    let title = title.split("[Ref ").next().unwrap_or(title);
    let mut normalized = String::new();
    let mut previous_was_digit = false;

    for character in title.chars() {
        if character.is_ascii_digit() {
            if !previous_was_digit {
                normalized.push('#');
                previous_was_digit = true;
            }
        } else {
            previous_was_digit = false;

            if character.is_whitespace() {
                if !normalized.ends_with(' ') {
                    normalized.push(' ');
                }
            } else {
                normalized.push(character);
            }
        }

        if normalized.len() >= 300 {
            break;
        }
    }

    normalized.trim().to_owned()
}

fn top_frame(entry: &LogEntry) -> String {
    entry
        .details
        .iter()
        .find_map(|line| {
            let name = frame_type_name(line)?;

            (!is_framework_type(&name)).then_some(name)
        })
        .or_else(|| entry.category.clone())
        .unwrap_or_default()
}

fn frame_type_name(line: &str) -> Option<String> {
    let trimmed = line.trim();

    if let Some(rest) = trimmed.strip_prefix("at ") {
        let rest = rest.trim_start_matches('.');
        let head = rest
            .split(['(', ' '])
            .next()?
            .split('+')
            .next()?
            .trim_end_matches('.');

        if head.is_empty() || !head.contains('.') {
            return None;
        }

        return Some(head.to_owned());
    }

    let (type_name, rest) = trimmed.split_once(':')?;

    if type_name.is_empty()
        || type_name.starts_with('[')
        || type_name.contains(char::is_whitespace)
        || !type_name.contains('.')
    {
        return None;
    }

    let method = rest.split([' ', '(']).next().unwrap_or_default();
    let looks_like_method = !method.is_empty()
        && method
            .chars()
            .all(|character| character.is_alphanumeric() || matches!(character, '_' | '<' | '>'));

    looks_like_method.then(|| type_name.trim_start_matches('.').to_owned())
}

fn type_candidates(entry: &LogEntry) -> Vec<String> {
    let mut candidates = Vec::new();

    for line in &entry.details {
        let Some(type_name) = frame_type_name(line) else {
            continue;
        };

        let mut name = type_name.trim_start_matches('.').to_owned();

        if let Some(position) = name.find('+') {
            name.truncate(position);
        }

        if let Some(position) = name.find('<') {
            name.truncate(position);
        }

        let name = name.trim_end_matches('.');

        if name.is_empty() || is_framework_type(name) {
            continue;
        }

        let parts = name
            .split('.')
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>();

        for take in (1..=parts.len()).rev() {
            let candidate = parts[..take].join(".");

            if candidate.len() >= 4 && !is_framework_type(&candidate) {
                candidates.push(candidate);
            }
        }

        if candidates.len() >= 16 {
            break;
        }
    }

    candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.len()));
    candidates.dedup();
    candidates.truncate(12);

    candidates
}

fn is_framework_type(name: &str) -> bool {
    const ROOTS: &[&str] = &[
        "System",
        "Microsoft",
        "Mono",
        "Mono.Cecil",
        "UnityEngine",
        "Unity",
        "Verse",
        "RimWorld",
        "RimWorldWaylith",
        "LudeonTK",
        "Brutal",
        "Waylith",
        "HarmonyLib",
        "Harmony",
        "0Harmony",
        "Newtonsoft",
        "Steamworks",
        "NAudio",
        "JetBrains",
        "DG",
        "TMPro",
        "Cecil",
        "I2",
    ];

    let root = name
        .trim_start_matches('.')
        .split('.')
        .next()
        .unwrap_or_default();

    ROOTS.contains(&root)
}

struct CodeIndex {
    resolved: HashMap<String, Vec<usize>>,
}

impl CodeIndex {
    /// Scan every mod assembly once for the namespace candidates collected
    /// from the log. Scanning all assemblies for every issue would be far too
    /// expensive, so this is a single pass over the modlist.
    fn build(mods: &[ModSnapshot], candidates: &[String]) -> Self {
        let mut buckets: Vec<Vec<usize>> = vec![Vec::new(); 256];

        for (index, candidate) in candidates.iter().enumerate() {
            if let Some(first) = candidate.as_bytes().first() {
                buckets[*first as usize].push(index);
            }
        }

        let mut resolved = candidates
            .iter()
            .map(|candidate| (candidate.clone(), Vec::new()))
            .collect::<HashMap<_, _>>();

        for (mod_index, rimworld_mod) in mods.iter().enumerate() {
            if rimworld_mod.assembly_paths.is_empty() {
                continue;
            }

            let mut found = vec![false; candidates.len()];

            for path in &rimworld_mod.assembly_paths {
                let Ok(metadata) = fs::metadata(path) else {
                    continue;
                };

                if metadata.len() > MAX_DLL_BYTES {
                    continue;
                }

                let Ok(bytes) = fs::read(path) else {
                    continue;
                };

                for (position, byte) in bytes.iter().enumerate() {
                    for &candidate_index in &buckets[*byte as usize] {
                        if found[candidate_index] {
                            continue;
                        }

                        if bytes[position..].starts_with(candidates[candidate_index].as_bytes()) {
                            found[candidate_index] = true;
                            resolved
                                .get_mut(&candidates[candidate_index])
                                .expect("candidate was inserted above")
                                .push(mod_index);
                        }
                    }
                }

                if found.iter().all(|&value| value) {
                    break;
                }
            }
        }

        Self { resolved }
    }

    fn attribute(
        &self,
        candidates: &[String],
        mods: &[ModSnapshot],
        builders: &mut HashMap<usize, SuspectBuilder>,
    ) {
        let mut matched = 0;
        let mut already_attributed = HashSet::new();

        for candidate in candidates {
            let Some(mod_indices) = self.resolved.get(candidate) else {
                continue;
            };

            if mod_indices.is_empty() {
                continue;
            }

            let shared = mod_indices.len() > 1;

            for &mod_index in mod_indices {
                if !already_attributed.insert(mod_index) {
                    continue;
                }

                let mut score = if shared { 55 } else { 70 };
                let root = candidate
                    .split('.')
                    .next()
                    .unwrap_or(candidate)
                    .to_lowercase();

                if mods[mod_index]
                    .assembly_stems
                    .iter()
                    .any(|stem| stem == &root || candidate.to_lowercase().starts_with(stem))
                {
                    score = 90;
                }

                let evidence = if shared {
                    format!("Stack frame namespace `{candidate}` is shared by several mods")
                } else {
                    format!("Stack frame namespace `{candidate}` is in this mod's assembly")
                };

                push_suspect(builders, mod_index, score, evidence);
            }

            matched += 1;

            if matched >= 3 {
                break;
            }
        }
    }
}

struct DefIndex {
    resolved: HashMap<String, Vec<usize>>,
}

impl DefIndex {
    fn build(mods: &[ModSnapshot], candidates: &[String]) -> Self {
        let candidate_set = candidates.iter().cloned().collect::<HashSet<_>>();
        let mut resolved = candidates
            .iter()
            .map(|candidate| (candidate.clone(), Vec::new()))
            .collect::<HashMap<_, _>>();
        let mut scanned_bytes = 0u64;

        for (mod_index, rimworld_mod) in mods.iter().enumerate() {
            let mut xml_files = Vec::new();
            collect_xml_files(&rimworld_mod.folder, &mut xml_files);

            for path in xml_files {
                if scanned_bytes >= MAX_DEF_BYTES {
                    return Self { resolved };
                }

                let Ok(metadata) = fs::metadata(&path) else {
                    continue;
                };

                if metadata.len() > MAX_XML_FILE_BYTES {
                    continue;
                }

                let Ok(bytes) = fs::read(&path) else {
                    continue;
                };

                scanned_bytes += bytes.len() as u64;
                scan_def_names(&bytes, &candidate_set, mod_index, &mut resolved);
            }
        }

        Self { resolved }
    }

    fn attribute(&self, candidates: &[String], builders: &mut HashMap<usize, SuspectBuilder>) {
        let mut matched = 0;
        let mut already_attributed = HashSet::new();

        for candidate in candidates {
            let Some(mod_indices) = self.resolved.get(candidate) else {
                continue;
            };

            for &mod_index in mod_indices {
                if !already_attributed.insert(mod_index) {
                    continue;
                }

                push_suspect(
                    builders,
                    mod_index,
                    65,
                    format!("Def `{candidate}` is defined in this mod"),
                );
            }

            if !mod_indices.is_empty() {
                matched += 1;
            }

            if matched >= 2 {
                break;
            }
        }
    }
}

fn collect_xml_files(folder: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };

    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let path = entry.path();

        if file_type.is_dir() {
            let is_skipped = entry.file_name().to_str().is_some_and(|name| {
                matches!(
                    name.to_ascii_lowercase().as_str(),
                    "about" | "assemblies" | "languages" | ".git" | "sounds" | "textures"
                )
            });

            if !is_skipped {
                collect_xml_files(&path, files);
            }
        } else if file_type.is_file()
            && path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("xml"))
        {
            files.push(path);
        }
    }
}

fn scan_def_names(
    bytes: &[u8],
    candidates: &HashSet<String>,
    mod_index: usize,
    resolved: &mut HashMap<String, Vec<usize>>,
) {
    const MARKER: &[u8] = b"<defName";

    let mut search_from = 0;

    while search_from < bytes.len() {
        let Some(relative) = find_subslice(&bytes[search_from..], MARKER) else {
            break;
        };
        let marker_position = search_from + relative;
        let after_marker = marker_position + MARKER.len();
        let Some(relative_close) = bytes[after_marker..].iter().position(|byte| *byte == b'>')
        else {
            break;
        };
        let value_start = after_marker + relative_close + 1;
        let Some(relative_value_end) = bytes[value_start..].iter().position(|byte| *byte == b'<')
        else {
            break;
        };
        let value_end = value_start + relative_value_end;
        let value = trim_ascii(&bytes[value_start..value_end]);

        if let Ok(value) = std::str::from_utf8(value)
            && candidates.contains(value)
        {
            let entries = resolved
                .get_mut(value)
                .expect("candidate was inserted by the caller");

            if !entries.contains(&mod_index) {
                entries.push(mod_index);
            }
        }

        search_from = value_end;
    }
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn trim_ascii(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|byte| !byte.is_ascii_whitespace())
        .map_or(start, |position| position + 1);

    &bytes[start..end]
}

fn def_candidates_from(entry: &LogEntry) -> Vec<String> {
    let mut candidates = Vec::new();

    for marker in ["Full node:", "Context:"] {
        let Some(position) = entry.message.find(marker) else {
            continue;
        };

        let context = &entry.message[position + marker.len()..];
        let context = context.chars().take(600).collect::<String>();

        for token in context.split(|character: char| {
            character.is_whitespace()
                || matches!(
                    character,
                    ',' | ';' | '(' | ')' | '[' | ']' | '"' | '\'' | '{' | '}' | ':' | '='
                )
        }) {
            let token = token.trim_matches(|character: char| {
                !character.is_ascii_alphanumeric() && !matches!(character, '_' | '-' | '.')
            });

            if token.len() < 4
                || token.len() > 80
                || !token
                    .chars()
                    .next()
                    .is_some_and(|character| character.is_ascii_alphabetic() || character == '_')
                || !token.chars().all(|character| {
                    character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.')
                })
                || DEF_STOPWORDS.contains(&token.to_ascii_lowercase().as_str())
            {
                continue;
            }

            candidates.push(token.to_owned());

            if candidates.len() >= 16 {
                break;
            }
        }
    }

    candidates.sort();
    candidates.dedup();
    candidates
}

fn is_non_culprit_suspect(suspect: &ModSuspect) -> bool {
    suspect.official || NON_CULPRIT_PACKAGE_IDS.contains(&suspect.package_id.as_str())
}

fn summarize_suspects(issues: &[LogIssue], mods: &[ModSnapshot]) -> Vec<SuspectSummary> {
    let mut summaries = HashMap::<usize, (usize, usize, u32)>::new();

    for issue in issues {
        if issue.level < LogLevel::Warning {
            continue;
        }

        let mut seen = HashSet::new();

        for suspect in &issue.suspects {
            let Some(mod_index) = mods
                .iter()
                .position(|rimworld_mod| rimworld_mod.package_id == suspect.package_id)
            else {
                continue;
            };

            if !seen.insert(mod_index) {
                continue;
            }

            let entry = summaries.entry(mod_index).or_insert((0, 0, 0));

            if issue.level == LogLevel::Error {
                entry.0 += 1;
            } else {
                entry.1 += 1;
            }

            entry.2 += suspect.score;
        }
    }

    let mut summaries = summaries
        .into_iter()
        .filter_map(|(mod_index, (errors, warnings, score))| {
            let rimworld_mod = mods.get(mod_index)?;

            // Base game, DLC and shared tooling are not modlist culprits. They
            // can still appear on a specific issue, but they must not dominate
            // the ranking.
            if rimworld_mod.official
                || NON_CULPRIT_PACKAGE_IDS.contains(&rimworld_mod.package_id.as_str())
            {
                return None;
            }

            Some(SuspectSummary {
                package_id: rimworld_mod.package_id.clone(),
                name: rimworld_mod.name.clone(),
                enabled: rimworld_mod.enabled,
                errors,
                warnings,
                issues: errors + warnings,
                score,
            })
        })
        .collect::<Vec<_>>();

    summaries.sort_by(|left, right| {
        right
            .errors
            .cmp(&left.errors)
            .then_with(|| right.issues.cmp(&left.issues))
            .then_with(|| right.score.cmp(&left.score))
            .then_with(|| left.name.cmp(&right.name))
    });

    summaries
}

#[cfg(test)]
#[path = "../../tests/unit/log_analysis.rs"]
mod tests;
