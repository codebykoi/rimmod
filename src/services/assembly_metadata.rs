use std::fs;
use std::path::Path;

/// Basic identity and reference metadata extracted statically from a .NET
/// (ECMA-335) assembly without loading or executing any code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AssemblyMetadata {
    /// Assembly name from the `Assembly` metadata table (e.g. `"HugsLib"`).
    pub(crate) name: String,

    /// Assembly names referenced in the `AssemblyRef` metadata table.
    pub(crate) references: Vec<String>,
}

/// Read assembly metadata from a `.dll` file on disk.
///
/// Returns `None` for unreadable files, native (non-.NET) DLLs, or malformed
/// metadata so assembly inspection can never break mod loading.
pub(crate) fn read_assembly_metadata_file(path: &Path) -> Option<AssemblyMetadata> {
    let bytes = fs::read(path).ok()?;
    read_assembly_metadata(&bytes)
}

/// Parse a PE32/PE32+ image and extract its ECMA-335 `Assembly` name and
/// `AssemblyRef` names.
pub(crate) fn read_assembly_metadata(bytes: &[u8]) -> Option<AssemblyMetadata> {
    let cli_offset = find_cli_header_offset(bytes)?;

    // IMAGE_COR20_HEADER (ECMA-335 §II.25.3.3):
    // +0: cb (u32), +8: MetaData RVA (u32), +12: MetaData Size (u32)
    let cb = read_u32(bytes, cli_offset)?;
    if cb < 16 {
        return None;
    }

    let metadata_rva = read_u32(bytes, cli_offset.checked_add(8)?)?;
    let metadata_size = read_u32(bytes, cli_offset.checked_add(12)?)? as usize;
    if metadata_rva == 0 || metadata_size < 16 {
        return None;
    }

    let sections = parse_sections(bytes)?;
    let metadata_offset = rva_to_offset(&sections, metadata_rva)?;
    let metadata = bytes.get(metadata_offset..metadata_offset.checked_add(metadata_size)?)?;

    parse_cli_metadata(metadata)
}

#[derive(Clone, Copy)]
struct PeSection {
    virtual_size: u32,
    virtual_address: u32,
    raw_data_size: u32,
    raw_data_ptr: u32,
}

fn pe_header_offset(bytes: &[u8]) -> Option<usize> {
    if bytes.get(0..2) != Some(b"MZ") {
        return None;
    }

    let offset = read_u32(bytes, 0x3C)? as usize;
    if bytes.get(offset..offset.checked_add(4)?) != Some(b"PE\0\0") {
        return None;
    }

    Some(offset)
}

fn parse_sections(bytes: &[u8]) -> Option<Vec<PeSection>> {
    let pe_offset = pe_header_offset(bytes)?;
    let coff_offset = pe_offset.checked_add(4)?;
    let num_sections = read_u16(bytes, coff_offset.checked_add(2)?)? as usize;
    let opt_header_size = read_u16(bytes, coff_offset.checked_add(16)?)? as usize;
    let sections_offset = coff_offset.checked_add(20)?.checked_add(opt_header_size)?;

    let mut sections = Vec::with_capacity(num_sections.min(128));

    for index in 0..num_sections {
        let entry = sections_offset.checked_add(index.checked_mul(40)?)?;
        let virtual_size = read_u32(bytes, entry.checked_add(8)?)?;
        let virtual_address = read_u32(bytes, entry.checked_add(12)?)?;
        let raw_data_size = read_u32(bytes, entry.checked_add(16)?)?;
        let raw_data_ptr = read_u32(bytes, entry.checked_add(20)?)?;

        sections.push(PeSection {
            virtual_size,
            virtual_address,
            raw_data_size,
            raw_data_ptr,
        });
    }

    Some(sections)
}

fn find_cli_header_offset(bytes: &[u8]) -> Option<usize> {
    let pe_offset = pe_header_offset(bytes)?;
    let coff_offset = pe_offset.checked_add(4)?;
    let opt_header_size = read_u16(bytes, coff_offset.checked_add(16)?)? as usize;
    let opt_offset = coff_offset.checked_add(20)?;
    let opt_end = opt_offset.checked_add(opt_header_size)?;

    let magic = read_u16(bytes, opt_offset)?;
    let (rva_count_rel, data_dir_rel) = match magic {
        // PE32 (standard for AnyCPU and x86 .NET assemblies)
        0x010B => (92usize, 96usize),
        // PE32+ (64-bit .NET assemblies)
        0x020B => (108usize, 112usize),
        _ => return None,
    };

    let rva_count_pos = opt_offset.checked_add(rva_count_rel)?;
    let data_dir_pos = opt_offset.checked_add(data_dir_rel)?;
    if data_dir_pos > opt_end {
        return None;
    }

    // Data directory 14 is the CLR Runtime Header (IMAGE_DIRECTORY_ENTRY_COM_DESCRIPTOR).
    const CLR_DIR_INDEX: usize = 14;
    let num_data_dirs = read_u32(bytes, rva_count_pos)? as usize;
    if num_data_dirs <= CLR_DIR_INDEX {
        return None;
    }

    let clr_dir_pos = data_dir_pos.checked_add(CLR_DIR_INDEX * 8)?;
    if clr_dir_pos.checked_add(8)? > opt_end {
        return None;
    }

    let clr_rva = read_u32(bytes, clr_dir_pos)?;
    let clr_size = read_u32(bytes, clr_dir_pos.checked_add(4)?)?;
    if clr_rva == 0 || clr_size < 16 {
        return None;
    }

    let sections = parse_sections(bytes)?;
    rva_to_offset(&sections, clr_rva)
}

fn rva_to_offset(sections: &[PeSection], rva: u32) -> Option<usize> {
    for section in sections {
        let span = section.virtual_size.max(section.raw_data_size);
        if rva >= section.virtual_address {
            let delta = rva - section.virtual_address;
            if delta < span && delta < section.raw_data_size {
                return section.raw_data_ptr.checked_add(delta).map(|o| o as usize);
            }
        }
    }

    None
}

fn parse_cli_metadata(metadata: &[u8]) -> Option<AssemblyMetadata> {
    // Metadata root signature "BSJB" (ECMA-335 §II.24.2.1).
    if metadata.get(0..4) != Some(b"BSJB") {
        return None;
    }

    let version_len = read_u32(metadata, 12)? as usize;
    let aligned_version_len = (version_len.checked_add(3)?) & !3;
    let mut cursor = 16usize.checked_add(aligned_version_len)?;

    // Flags (u16) + Number of streams (u16).
    let _flags = read_u16(metadata, cursor)?;
    let num_streams = read_u16(metadata, cursor.checked_add(2)?)? as usize;
    cursor = cursor.checked_add(4)?;

    let mut tables_stream: Option<&[u8]> = None;
    let mut strings_heap: Option<&[u8]> = None;

    for _ in 0..num_streams {
        let stream_offset = read_u32(metadata, cursor)? as usize;
        let stream_size = read_u32(metadata, cursor.checked_add(4)?)? as usize;
        let name_start = cursor.checked_add(8)?;

        // Stream names are short null-terminated ASCII strings padded to 4 bytes.
        let search_end = name_start.checked_add(64)?.min(metadata.len());
        let relative_null = metadata
            .get(name_start..search_end)?
            .iter()
            .position(|&byte| byte == 0)?;
        let name = metadata.get(name_start..name_start + relative_null)?;
        let padded_name_len = ((relative_null + 1) + 3) & !3;
        cursor = name_start.checked_add(padded_name_len)?;

        let stream_slice = metadata.get(stream_offset..stream_offset.checked_add(stream_size)?)?;

        match name {
            b"#~" | b"#-" => tables_stream = Some(stream_slice),
            b"#Strings" => strings_heap = Some(stream_slice),
            _ => {}
        }
    }

    parse_metadata_tables(tables_stream?, strings_heap?)
}

fn parse_metadata_tables(tables: &[u8], strings_heap: &[u8]) -> Option<AssemblyMetadata> {
    // #~ / #- stream header (ECMA-335 §II.24.2.6):
    // +0: Reserved (4), +4: Major (1), +5: Minor (1), +6: HeapSizes (1), +7: Reserved (1)
    // +8: Valid (8), +16: Sorted (8)
    let heap_sizes = *tables.get(6)?;
    let valid = read_u64(tables, 8)?;

    let mut cursor = 24usize;
    let mut row_counts = [0u32; 64];

    for (table_id, slot) in row_counts.iter_mut().enumerate() {
        if (valid & (1u64 << table_id)) != 0 {
            *slot = read_u32(tables, cursor)?;
            cursor = cursor.checked_add(4)?;
        }
    }

    // Some uncompressed (#-) streams include an extra 4-byte field when bit 0x40 is set.
    if (heap_sizes & 0x40) != 0 {
        cursor = cursor.checked_add(4)?;
    }

    let str_sz = if (heap_sizes & 0x01) != 0 { 4 } else { 2 };
    let blob_sz = if (heap_sizes & 0x04) != 0 { 4 } else { 2 };

    let mut assembly_name: Option<String> = None;
    let mut references = Vec::new();

    // Walk metadata tables 0x00..=0x23 so we land directly on Assembly (0x20)
    // and AssemblyRef (0x23) without needing definitions for later tables.
    for table_id in 0x00usize..=0x23usize {
        if (valid & (1u64 << table_id)) == 0 {
            continue;
        }

        let rows = row_counts[table_id] as usize;
        let row_size = table_row_size(table_id, heap_sizes, &row_counts)?;
        let table_bytes = rows.checked_mul(row_size)?;
        let table_data = tables.get(cursor..cursor.checked_add(table_bytes)?)?;
        cursor = cursor.checked_add(table_bytes)?;

        if table_id == 0x20 && rows > 0 {
            // Assembly row: HashAlgId(4) + Version(8) + Flags(4) + PublicKey(blob) + Name(str) + Culture(str)
            let name_offset = 16 + blob_sz;
            let name_index = read_index(table_data, name_offset, str_sz)?;
            if let Some(name) = read_heap_string(strings_heap, name_index)
                && !name.is_empty()
            {
                assembly_name = Some(name);
            }
        } else if table_id == 0x23 {
            // AssemblyRef row: Version(8) + Flags(4) + PublicKeyOrToken(blob) + Name(str) + Culture(str) + HashValue(blob)
            let name_offset = 12 + blob_sz;
            for row_index in 0..rows {
                let row_start = row_index * row_size;
                let name_index = read_index(table_data, row_start + name_offset, str_sz)?;
                if let Some(name) = read_heap_string(strings_heap, name_index)
                    && !name.is_empty()
                    && !references
                        .iter()
                        .any(|existing: &String| existing.eq_ignore_ascii_case(&name))
                {
                    references.push(name);
                }
            }
        }
    }

    Some(AssemblyMetadata {
        name: assembly_name?,
        references,
    })
}

fn table_row_size(table_id: usize, heap_sizes: u8, row_counts: &[u32; 64]) -> Option<usize> {
    let str_sz = if (heap_sizes & 0x01) != 0 { 4 } else { 2 };
    let guid_sz = if (heap_sizes & 0x02) != 0 { 4 } else { 2 };
    let blob_sz = if (heap_sizes & 0x04) != 0 { 4 } else { 2 };

    let tbl = |id: usize| -> usize { if row_counts[id] < 65_536 { 2 } else { 4 } };

    let coded = |tables: &[usize], tag_bits: u32| -> usize {
        let limit = 1u32 << (16 - tag_bits);
        let max_rows = tables.iter().map(|&id| row_counts[id]).max().unwrap_or(0);
        if max_rows < limit { 2 } else { 4 }
    };

    // ECMA-335 §II.24.2.6 coded index table sets.
    let type_def_or_ref = coded(&[0x02, 0x01, 0x1B], 2);
    let has_constant = coded(&[0x04, 0x08, 0x17], 2);
    let has_custom_attribute = coded(
        &[
            0x06, 0x04, 0x01, 0x02, 0x08, 0x09, 0x0A, 0x00, 0x0E, 0x17, 0x14, 0x11, 0x1A, 0x1B,
            0x20, 0x23, 0x26, 0x27, 0x28, 0x2A, 0x2C, 0x2B,
        ],
        5,
    );
    let has_field_marshal = coded(&[0x04, 0x08], 1);
    let has_decl_security = coded(&[0x02, 0x06, 0x20], 2);
    let member_ref_parent = coded(&[0x02, 0x01, 0x1A, 0x06, 0x1B], 3);
    let has_semantics = coded(&[0x14, 0x17], 1);
    let method_def_or_ref = coded(&[0x06, 0x0A], 1);
    let member_forwarded = coded(&[0x04, 0x06], 1);
    let custom_attribute_type = coded(&[0x06, 0x0A], 3);
    let resolution_scope = coded(&[0x00, 0x1A, 0x23, 0x01], 2);

    let size = match table_id {
        0x00 => 2 + str_sz + 3 * guid_sz,      // Module
        0x01 => resolution_scope + 2 * str_sz, // TypeRef
        0x02 => 4 + 2 * str_sz + type_def_or_ref + tbl(0x04) + tbl(0x06), // TypeDef
        0x03 => tbl(0x04),                     // FieldPtr
        0x04 => 2 + str_sz + blob_sz,          // Field
        0x05 => tbl(0x06),                     // MethodPtr
        0x06 => 4 + 2 + 2 + str_sz + blob_sz + tbl(0x08), // MethodDef
        0x07 => tbl(0x08),                     // ParamPtr
        0x08 => 2 + 2 + str_sz,                // Param
        0x09 => tbl(0x02) + type_def_or_ref,   // InterfaceImpl
        0x0A => member_ref_parent + str_sz + blob_sz, // MemberRef
        0x0B => 2 + has_constant + blob_sz,    // Constant
        0x0C => has_custom_attribute + custom_attribute_type + blob_sz, // CustomAttribute
        0x0D => has_field_marshal + blob_sz,   // FieldMarshal
        0x0E => 2 + has_decl_security + blob_sz, // DeclSecurity
        0x0F => 2 + 4 + tbl(0x02),             // ClassLayout
        0x10 => 4 + tbl(0x04),                 // FieldLayout
        0x11 => blob_sz,                       // StandAloneSig
        0x12 => tbl(0x02) + tbl(0x14),         // EventMap
        0x13 => tbl(0x14),                     // EventPtr
        0x14 => 2 + str_sz + type_def_or_ref,  // Event
        0x15 => tbl(0x02) + tbl(0x17),         // PropertyMap
        0x16 => tbl(0x17),                     // PropertyPtr
        0x17 => 2 + str_sz + blob_sz,          // Property
        0x18 => 2 + tbl(0x06) + has_semantics, // MethodSemantics
        0x19 => tbl(0x02) + 2 * method_def_or_ref, // MethodImpl
        0x1A => str_sz,                        // ModuleRef
        0x1B => blob_sz,                       // TypeSpec
        0x1C => 2 + member_forwarded + str_sz + tbl(0x1A), // ImplMap
        0x1D => 4 + tbl(0x04),                 // FieldRVA
        0x1E => 4 + 4,                         // ENCLog
        0x1F => 4,                             // ENCMap
        0x20 => 16 + blob_sz + 2 * str_sz,     // Assembly
        0x21 => 4,                             // AssemblyProcessor
        0x22 => 4 + 4 + 4,                     // AssemblyOS
        0x23 => 12 + 2 * blob_sz + 2 * str_sz, // AssemblyRef
        _ => return None,
    };

    Some(size)
}

fn read_index(bytes: &[u8], offset: usize, size: usize) -> Option<usize> {
    match size {
        2 => read_u16(bytes, offset).map(|value| value as usize),
        4 => read_u32(bytes, offset).map(|value| value as usize),
        _ => None,
    }
}

fn read_heap_string(strings_heap: &[u8], offset: usize) -> Option<String> {
    let tail = strings_heap.get(offset..)?;
    let null_pos = tail.iter().position(|&byte| byte == 0)?;
    let raw = std::str::from_utf8(&tail[..null_pos]).ok()?;
    Some(raw.trim().to_owned())
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    let slice = bytes.get(offset..offset.checked_add(2)?)?;
    Some(u16::from_le_bytes([slice[0], slice[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let slice = bytes.get(offset..offset.checked_add(4)?)?;
    Some(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn read_u64(bytes: &[u8], offset: usize) -> Option<u64> {
    let slice = bytes.get(offset..offset.checked_add(8)?)?;
    Some(u64::from_le_bytes([
        slice[0], slice[1], slice[2], slice[3], slice[4], slice[5], slice[6], slice[7],
    ]))
}

#[cfg(test)]
#[path = "../../tests/unit/assembly_metadata.rs"]
pub(crate) mod tests;
