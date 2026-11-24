use super::*;

/// Build a minimal, spec-valid synthetic PE32 or PE32+ .NET assembly containing
/// `Module`, `TypeRef`, `Assembly`, and `AssemblyRef` metadata tables.
pub(crate) fn build_synthetic_dotnet_dll(
    pe32_plus: bool,
    assembly_name: &str,
    references: &[&str],
) -> Vec<u8> {
    // Build #Strings heap (offset 0 is always the empty string).
    let mut strings = vec![0u8];
    let mut add_string = |value: &str| -> u16 {
        let offset = strings.len() as u16;
        strings.extend_from_slice(value.as_bytes());
        strings.push(0);
        offset
    };

    let module_name_idx = add_string("Synthetic.dll");
    let typeref_name_idx = add_string("Object");
    let typeref_ns_idx = add_string("System");
    let asm_name_idx = add_string(assembly_name);
    let ref_indices: Vec<u16> = references.iter().map(|name| add_string(name)).collect();

    while strings.len() % 4 != 0 {
        strings.push(0);
    }

    // Build #~ tables stream with Module (0x00), TypeRef (0x01), Assembly (0x20),
    // and optionally AssemblyRef (0x23) to exercise skipping earlier tables.
    let mut valid: u64 = (1 << 0x00) | (1 << 0x01) | (1 << 0x20);
    if !ref_indices.is_empty() {
        valid |= 1 << 0x23;
    }

    let mut tables = Vec::new();
    tables.extend_from_slice(&0u32.to_le_bytes()); // Reserved
    tables.push(2); // MajorVersion
    tables.push(0); // MinorVersion
    tables.push(0); // HeapSizes (2-byte indices)
    tables.push(1); // Reserved
    tables.extend_from_slice(&valid.to_le_bytes());
    tables.extend_from_slice(&0u64.to_le_bytes()); // Sorted

    // Row counts for each valid table bit.
    tables.extend_from_slice(&1u32.to_le_bytes()); // Module: 1
    tables.extend_from_slice(&1u32.to_le_bytes()); // TypeRef: 1
    tables.extend_from_slice(&1u32.to_le_bytes()); // Assembly: 1
    if !ref_indices.is_empty() {
        tables.extend_from_slice(&(ref_indices.len() as u32).to_le_bytes()); // AssemblyRef
    }

    // Table 0x00: Module (10 bytes: Generation(2) + Name(2) + 3*Guid(2))
    tables.extend_from_slice(&0u16.to_le_bytes());
    tables.extend_from_slice(&module_name_idx.to_le_bytes());
    tables.extend_from_slice(&[0u8; 6]);

    // Table 0x01: TypeRef (6 bytes: ResolutionScope(2) + Name(2) + Namespace(2))
    tables.extend_from_slice(&0u16.to_le_bytes());
    tables.extend_from_slice(&typeref_name_idx.to_le_bytes());
    tables.extend_from_slice(&typeref_ns_idx.to_le_bytes());

    // Table 0x20: Assembly (22 bytes: HashAlgId(4) + Ver(8) + Flags(4) + PublicKey(2) + Name(2) + Culture(2))
    tables.extend_from_slice(&0x8004u32.to_le_bytes());
    tables.extend_from_slice(&[0u8; 8]);
    tables.extend_from_slice(&0u32.to_le_bytes());
    tables.extend_from_slice(&0u16.to_le_bytes());
    tables.extend_from_slice(&asm_name_idx.to_le_bytes());
    tables.extend_from_slice(&0u16.to_le_bytes());

    // Table 0x23: AssemblyRef (20 bytes each: Ver(8) + Flags(4) + PublicKeyOrToken(2) + Name(2) + Culture(2) + HashValue(2))
    for ref_idx in ref_indices {
        tables.extend_from_slice(&[0u8; 8]);
        tables.extend_from_slice(&0u32.to_le_bytes());
        tables.extend_from_slice(&0u16.to_le_bytes());
        tables.extend_from_slice(&ref_idx.to_le_bytes());
        tables.extend_from_slice(&0u16.to_le_bytes());
        tables.extend_from_slice(&0u16.to_le_bytes());
    }

    while tables.len() % 4 != 0 {
        tables.push(0);
    }

    // Metadata root ("BSJB").
    let version = b"v4.0.30319\0\0"; // 12 bytes (4-byte aligned)
    let mut metadata = Vec::new();
    metadata.extend_from_slice(b"BSJB");
    metadata.extend_from_slice(&1u16.to_le_bytes()); // Major
    metadata.extend_from_slice(&1u16.to_le_bytes()); // Minor
    metadata.extend_from_slice(&0u32.to_le_bytes()); // Reserved
    metadata.extend_from_slice(&(version.len() as u32).to_le_bytes());
    metadata.extend_from_slice(version);
    metadata.extend_from_slice(&0u16.to_le_bytes()); // Flags
    metadata.extend_from_slice(&2u16.to_le_bytes()); // 2 streams: #~ and #Strings

    // Stream headers:
    // "#~\0\0" = 4 bytes -> header = 12 bytes
    // "#Strings\0\0\0\0" = 12 bytes -> header = 20 bytes
    let tables_offset = (metadata.len() + 12 + 20) as u32;
    let strings_offset = tables_offset + tables.len() as u32;

    metadata.extend_from_slice(&tables_offset.to_le_bytes());
    metadata.extend_from_slice(&(tables.len() as u32).to_le_bytes());
    metadata.extend_from_slice(b"#~\0\0");

    metadata.extend_from_slice(&strings_offset.to_le_bytes());
    metadata.extend_from_slice(&(strings.len() as u32).to_le_bytes());
    metadata.extend_from_slice(b"#Strings\0\0\0\0");

    metadata.extend_from_slice(&tables);
    metadata.extend_from_slice(&strings);

    // Put CLI header + metadata inside a single .text section at RVA 0x2000, file offset 0x200.
    let section_rva: u32 = 0x2000;
    let section_file_offset: u32 = 0x200;
    let cli_header_size: u32 = 72;
    let metadata_rva = section_rva + cli_header_size;

    let mut text_section = Vec::new();
    text_section.extend_from_slice(&cli_header_size.to_le_bytes()); // cb
    text_section.extend_from_slice(&2u16.to_le_bytes()); // MajorRuntimeVersion
    text_section.extend_from_slice(&5u16.to_le_bytes()); // MinorRuntimeVersion
    text_section.extend_from_slice(&metadata_rva.to_le_bytes());
    text_section.extend_from_slice(&(metadata.len() as u32).to_le_bytes());
    text_section.resize(cli_header_size as usize, 0);
    text_section.extend_from_slice(&metadata);

    // Build DOS + PE + COFF + Optional Header + Section Table.
    let pe_offset: u32 = 0x80;
    let opt_header_size: u16 = if pe32_plus { 240 } else { 224 };
    let mut image = vec![0u8; section_file_offset as usize];

    image[0..2].copy_from_slice(b"MZ");
    image[0x3C..0x40].copy_from_slice(&pe_offset.to_le_bytes());

    let pe = pe_offset as usize;
    image[pe..pe + 4].copy_from_slice(b"PE\0\0");
    // COFF: NumberOfSections = 1, SizeOfOptionalHeader
    image[pe + 6..pe + 8].copy_from_slice(&1u16.to_le_bytes());
    image[pe + 20..pe + 22].copy_from_slice(&opt_header_size.to_le_bytes());

    // Optional header magic + NumberOfRvaAndSizes + CLR DataDirectory[14].
    let opt = pe + 24;
    let (magic, rva_count_rel, data_dir_rel): (u16, usize, usize) = if pe32_plus {
        (0x020B, 108, 112)
    } else {
        (0x010B, 92, 96)
    };
    image[opt..opt + 2].copy_from_slice(&magic.to_le_bytes());
    image[opt + rva_count_rel..opt + rva_count_rel + 4].copy_from_slice(&16u32.to_le_bytes());
    let clr_dir = opt + data_dir_rel + 14 * 8;
    image[clr_dir..clr_dir + 4].copy_from_slice(&section_rva.to_le_bytes());
    image[clr_dir + 4..clr_dir + 8].copy_from_slice(&cli_header_size.to_le_bytes());

    // Section table entry 0 (.text).
    let sec = opt + opt_header_size as usize;
    image[sec..sec + 5].copy_from_slice(b".text");
    let raw_size = text_section.len() as u32;
    image[sec + 8..sec + 12].copy_from_slice(&raw_size.to_le_bytes()); // VirtualSize
    image[sec + 12..sec + 16].copy_from_slice(&section_rva.to_le_bytes()); // VirtualAddress
    image[sec + 16..sec + 20].copy_from_slice(&raw_size.to_le_bytes()); // SizeOfRawData
    image[sec + 20..sec + 24].copy_from_slice(&section_file_offset.to_le_bytes()); // PointerToRawData

    image.extend_from_slice(&text_section);
    image
}

#[test]
fn parses_pe32_assembly_name_and_references() {
    let bytes = build_synthetic_dotnet_dll(
        false,
        "ExampleAddon",
        &["mscorlib", "Assembly-CSharp", "HugsLib", "VEF"],
    );

    let metadata = read_assembly_metadata(&bytes).expect("synthetic PE32 should parse");

    assert_eq!(metadata.name, "ExampleAddon");
    assert_eq!(
        metadata.references,
        ["mscorlib", "Assembly-CSharp", "HugsLib", "VEF"]
    );
}

#[test]
fn parses_pe32_plus_assembly_name_and_references() {
    let bytes =
        build_synthetic_dotnet_dll(true, "SixtyFourBitMod", &["0Harmony", "CombatExtended"]);

    let metadata = read_assembly_metadata(&bytes).expect("synthetic PE32+ should parse");

    assert_eq!(metadata.name, "SixtyFourBitMod");
    assert_eq!(metadata.references, ["0Harmony", "CombatExtended"]);
}

#[test]
fn deduplicates_repeated_assembly_references_case_insensitively() {
    let bytes = build_synthetic_dotnet_dll(false, "DupRefs", &["HugsLib", "hugslib", "VEF"]);

    let metadata = read_assembly_metadata(&bytes).expect("assembly should parse");

    assert_eq!(metadata.references, ["HugsLib", "VEF"]);
}

#[test]
fn returns_none_for_native_dll_without_clr_header() {
    let mut bytes = build_synthetic_dotnet_dll(false, "NativeLike", &[]);
    // Zero out CLR DataDirectory[14] RVA and Size.
    let pe = 0x80usize;
    let clr_dir = pe + 24 + 96 + 14 * 8;
    bytes[clr_dir..clr_dir + 8].fill(0);

    assert!(read_assembly_metadata(&bytes).is_none());
}

#[test]
fn returns_none_for_invalid_or_truncated_bytes() {
    assert!(read_assembly_metadata(b"").is_none());
    assert!(read_assembly_metadata(b"not a dll at all").is_none());

    let full = build_synthetic_dotnet_dll(false, "Truncated", &["HugsLib"]);
    for cut in [2, 0x40, 0x100, 0x220, full.len() - 4] {
        assert!(read_assembly_metadata(&full[..cut]).is_none());
    }
}
