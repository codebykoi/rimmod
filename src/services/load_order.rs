use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use quick_xml::{
    events::{BytesText, Event},
    reader::Reader,
    writer::Writer,
};

use crate::{
    models::{ModCollection, PackageId},
    services::mod_loader::ModsConfigXml,
};

static NEXT_TEMPORARY_FILE: AtomicU64 = AtomicU64::new(0);

pub(crate) struct SaveLoadOrderOutcome {
    pub(crate) backup_path: PathBuf,
}

struct TemporaryFile {
    path: PathBuf,
}

impl TemporaryFile {
    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TemporaryFile {
    fn drop(&mut self) {
        // A successful rename removes the temporary path. On an error this
        // cleans up the incomplete file without hiding the original failure.
        let _ = fs::remove_file(&self.path);
    }
}

fn path_error(action: &str, path: &Path, error: io::Error) -> io::Error {
    io::Error::new(
        error.kind(),
        format!("{action} {}: {error}", path.display()),
    )
}

fn write_temporary_file(
    config_folder: &Path,
    purpose: &str,
    contents: &[u8],
) -> io::Result<TemporaryFile> {
    for _ in 0..100 {
        let unique_number = NEXT_TEMPORARY_FILE.fetch_add(1, Ordering::Relaxed);
        let path = config_folder.join(format!(
            ".ModsConfig.{purpose}.{}.{unique_number}.tmp",
            std::process::id()
        ));

        let mut file = match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(path_error("could not create temporary file", &path, error)),
        };
        let temporary_file = TemporaryFile { path };

        file.write_all(contents).map_err(|error| {
            path_error(
                "could not write temporary file",
                temporary_file.path(),
                error,
            )
        })?;
        file.sync_all().map_err(|error| {
            path_error(
                "could not sync temporary file",
                temporary_file.path(),
                error,
            )
        })?;

        // Close the handle before renaming. This is required on Windows.
        drop(file);

        return Ok(temporary_file);
    }

    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not create a unique temporary ModsConfig file",
    ))
}

fn parse_config_file(config_path: &Path) -> io::Result<Vec<PackageId>> {
    let xml = fs::read_to_string(config_path)
        .map_err(|error| path_error("could not read configuration", config_path, error))?;

    let config: ModsConfigXml = quick_xml::de::from_str(&xml).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("could not parse {}: {error}", config_path.display()),
        )
    })?;

    config
        .active_mods
        .package_ids
        .into_iter()
        .map(|raw_id| {
            PackageId::new(&raw_id).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{} contains an empty package ID", config_path.display()),
                )
            })
        })
        .collect()
}

pub(crate) fn parse_config(config_folder: &Path) -> io::Result<Vec<PackageId>> {
    let config_path = config_folder.join("ModsConfig.xml");
    parse_config_file(&config_path)
}

fn replace_active_mods(original_xml: &str, package_ids: &[&str]) -> io::Result<Vec<u8>> {
    let mut reader = Reader::from_str(original_xml);
    let mut writer = Writer::new(Vec::new());
    let mut found_active_mods = false;

    loop {
        let event = reader
            .read_event()
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;

        match event {
            Event::Start(start) if start.name().as_ref() == "activeMods" => {
                if found_active_mods {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "ModsConfig.xml contains multiple activeMods elements",
                    ));
                }

                found_active_mods = true;

                let end = start.to_end().into_owned();

                // Preserve the original opening tag.
                writer.write_event(Event::Start(start))?;

                // Discard the old <li> elements.
                reader
                    .read_to_end(end.name())
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;

                for package_id in package_ids {
                    writer.write_event(Event::Text(BytesText::new("\n    ")))?;

                    writer
                        .create_element("li")
                        .write_text_content(BytesText::new(package_id))?;
                }

                writer.write_event(Event::Text(BytesText::new("\n  ")))?;
                writer.write_event(Event::End(end))?;
            }

            Event::Eof => break,

            // Copy every unrelated part of the configuration unchanged.
            event => writer.write_event(event)?,
        }
    }

    if !found_active_mods {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "ModsConfig.xml does not contain activeMods",
        ));
    }

    Ok(writer.into_inner())
}

fn save_load_order_with_replace<F>(
    config_folder: &Path,
    mods: &ModCollection,
    replace_config: F,
) -> io::Result<SaveLoadOrderOutcome>
where
    F: FnOnce(&Path, &Path) -> io::Result<()>,
{
    if !mods.missing_active_package_ids().is_empty() {
        let missing_ids = mods
            .missing_active_package_ids()
            .iter()
            .map(PackageId::as_str)
            .collect::<Vec<_>>()
            .join(", ");

        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Cannot save because these active mods were not found: {missing_ids}"),
        ));
    }

    let active_package_ids = mods
        .enabled_ids()
        .iter()
        .map(|&index| {
            mods.get(index)
                .map(|rimworld_mod| rimworld_mod.package_id.as_str())
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "active mod index is invalid")
                })
        })
        .collect::<io::Result<Vec<_>>>()?;

    let config_file = config_folder.join("ModsConfig.xml");
    let original_xml = fs::read_to_string(&config_file)
        .map_err(|error| path_error("could not read configuration", &config_file, error))?;

    let updated_xml = replace_active_mods(&original_xml, &active_package_ids)?;
    let updated_file = write_temporary_file(config_folder, "new", &updated_xml)?;

    // Parse the completed file from disk, not only the in-memory buffer. This
    // verifies that the exact bytes waiting to replace ModsConfig.xml are valid.
    let validated_package_ids = parse_config_file(updated_file.path())?;
    let validated_ids = validated_package_ids
        .iter()
        .map(PackageId::as_str)
        .collect::<Vec<_>>();

    if validated_ids != active_package_ids {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "temporary configuration does not contain the requested load order",
        ));
    }

    let backup_file = config_folder.join("ModsConfig.rimmod-backup.xml");
    let backup_temp = write_temporary_file(config_folder, "backup", original_xml.as_bytes())?;

    // Replace the previous backup only after its complete contents are synced.
    fs::rename(backup_temp.path(), &backup_file)
        .map_err(|error| path_error("could not replace backup", &backup_file, error))?;

    // Both paths are in the same directory, allowing the operating system to
    // perform an atomic replacement on filesystems that support it.
    replace_config(updated_file.path(), &config_file)
        .map_err(|error| path_error("could not replace configuration", &config_file, error))?;

    Ok(SaveLoadOrderOutcome {
        backup_path: backup_file,
    })
}

pub(crate) fn save_load_order(
    config_folder: &Path,
    mods: &ModCollection,
) -> io::Result<SaveLoadOrderOutcome> {
    save_load_order_with_replace(config_folder, mods, |temporary_file, config_file| {
        fs::rename(temporary_file, config_file)
    })
}

#[cfg(test)]
#[path = "../../tests/unit/load_order.rs"]
mod tests;
