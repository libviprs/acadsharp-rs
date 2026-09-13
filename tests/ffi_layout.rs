//! Every struct on the boundary, laid out twice and compared.
//!
//! The header is the contract, so one side of the comparison is a parser that
//! reads `native/viprs_acadsharp.h` and applies the C layout rules to whatever
//! it finds there. The other side is what the compiler actually did to the
//! `#[repr(C)]` definitions in `acadsharp_rs::ffi`, read out with
//! `core::mem::offset_of!` and `size_of`.
//!
//! I compare the ordered list of `(field, offset, size)` rather than a set,
//! because swapping two fields of the same width leaves every offset exactly
//! where it was. Order is the only thing that moves, so order is what the
//! comparison keys on. A field in the wrong place does not fail to compile and
//! does not throw: it reads its neighbour's bytes, and a `uint64_t` read where
//! a `double` lives comes back as a plausible number.

use acadsharp_rs::ffi;

mod common;

/// One field as the header describes it, with the offset the C rules put it at.
#[derive(Debug, Clone, PartialEq, Eq)]
struct HeaderField {
    name: String,
    offset: usize,
    size: usize,
}

/// A whole struct as the header describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct HeaderStruct {
    name: String,
    fields: Vec<HeaderField>,
    size: usize,
    align: usize,
}

/// What the compiler did to the Rust definition.
#[derive(Debug)]
struct RustStruct {
    fields: Vec<(&'static str, usize, usize)>,
    size: usize,
    align: usize,
}

/// Size and alignment of the only four scalar types this boundary uses.
///
/// ABI.md pins the list at `uint8_t`, `uint32_t`, `uint64_t` and `double`, so
/// anything else is a change to the contract rather than a gap in this table,
/// and a panic is the right answer to it.
fn scalar(ty: &str) -> (usize, usize) {
    match ty {
        "uint8_t" => (1, 1),
        "uint16_t" => (2, 2),
        "uint32_t" => (4, 4),
        "uint64_t" => (8, 8),
        "double" => (8, 8),
        other => panic!(
            "the header uses `{other}`, which is not one of the four scalar types ABI.md allows on this boundary. \
             Either the contract changed or this parser is wrong, and both need a human."
        ),
    }
}

fn round_up(value: usize, align: usize) -> usize {
    value.div_ceil(align) * align
}

/// Every `struct NAME { ... };` the header defines a body for, laid out by the
/// C rules: each member at the next offset its own alignment allows, the whole
/// struct aligned to its widest member and rounded up to a multiple of that.
///
/// The two opaque handles are `typedef struct X X;` with no body, so they are
/// not picked up here, which is right: they have no layout to check.
fn structs_from_header() -> Vec<HeaderStruct> {
    let text = common::header_text();
    let source = common::strip_block_comments(&text);
    let lines: Vec<&str> = source.lines().collect();

    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        if !(trimmed.starts_with("struct ") && trimmed.ends_with('{')) {
            i += 1;
            continue;
        }
        let name = trimmed
            .trim_start_matches("struct ")
            .trim_end_matches('{')
            .trim()
            .to_string();

        let mut body = String::new();
        i += 1;
        let mut closed = false;
        while i < lines.len() {
            if lines[i].trim() == "};" {
                closed = true;
                i += 1;
                break;
            }
            body.push_str(lines[i]);
            body.push('\n');
            i += 1;
        }
        assert!(closed, "`struct {name}` in the header is never closed");

        let mut fields = Vec::new();
        let mut cursor = 0usize;
        let mut align = 1usize;
        for decl in body.split(';') {
            let decl = decl.trim();
            if decl.is_empty() {
                continue;
            }
            let tokens: Vec<&str> = decl.split_whitespace().collect();
            assert_eq!(
                tokens.len(),
                2,
                "`{decl}` in `struct {name}` is not a plain `type name` declaration, and this parser only handles those"
            );
            let (size, field_align) = scalar(tokens[0]);
            let offset = round_up(cursor, field_align);
            cursor = offset + size;
            align = align.max(field_align);
            fields.push(HeaderField {
                name: tokens[1].to_string(),
                offset,
                size,
            });
        }

        assert!(
            !fields.is_empty(),
            "`struct {name}` has no fields, so the parser found nothing to check"
        );
        out.push(HeaderStruct {
            name,
            size: round_up(cursor, align),
            align,
            fields,
        });
    }
    out
}

fn header_struct(name: &str) -> HeaderStruct {
    let all = structs_from_header();
    all.into_iter()
        .find(|s| s.name == name)
        .unwrap_or_else(|| panic!("the vendored header does not define `struct {name}`"))
}

/// Builds the Rust side of the comparison from nothing but the field
/// identifiers, so the field names in this file are the ones the compiler
/// checks rather than a second copy I could typo.
macro_rules! rust_struct {
    ($t:ty, $($f:ident),+ $(,)?) => {{
        let probe = <$t>::default();
        RustStruct {
            fields: vec![
                $((
                    stringify!($f),
                    core::mem::offset_of!($t, $f),
                    core::mem::size_of_val(&probe.$f),
                )),+
            ],
            size: core::mem::size_of::<$t>(),
            align: core::mem::align_of::<$t>(),
        }
    }};
}

fn compare(header: &HeaderStruct, rust: &RustStruct) {
    let name = &header.name;

    let header_names: Vec<&str> = header.fields.iter().map(|f| f.name.as_str()).collect();
    let rust_names: Vec<&str> = rust.fields.iter().map(|f| f.0).collect();
    assert_eq!(
        header_names, rust_names,
        "`{name}`: the header declares its fields in one order and `ffi.rs` declares them in another. \
         Header order: {header_names:?}. `ffi.rs` order: {rust_names:?}."
    );

    for (h, r) in header.fields.iter().zip(rust.fields.iter()) {
        assert_eq!(
            h.offset, r.1,
            "`{name}.{}` is at byte {} by the header's rules and at byte {} in `ffi.rs`",
            h.name, h.offset, r.1
        );
        assert_eq!(
            h.size, r.2,
            "`{name}.{}` is {} bytes wide in the header and {} bytes wide in `ffi.rs`",
            h.name, h.size, r.2
        );
    }

    assert_eq!(
        header.size, rust.size,
        "`{name}` is {} bytes by the header's rules and {} bytes in `ffi.rs`",
        header.size, rust.size
    );
    assert_eq!(
        header.align, rust.align,
        "`{name}` wants {}-byte alignment by the header's rules and got {} in `ffi.rs`",
        header.align, rust.align
    );
}

#[test]
fn the_parser_found_the_structs_it_was_meant_to_find() {
    // A layout test whose parser quietly found nothing passes, which is why
    // this one exists before the comparisons. Zero has two explanations and
    // only one of them is good.
    let found: Vec<String> = structs_from_header().into_iter().map(|s| s.name).collect();
    assert_eq!(
        found,
        vec![
            "viprs_acad_limits_v1".to_string(),
            "viprs_acad_capabilities_v1".to_string(),
            "viprs_acad_view_info_v1".to_string(),
        ],
        "the header defines a different set of structs than this test models"
    );
}

#[test]
fn every_struct_opens_with_struct_size_and_struct_version() {
    // ABI.md's first fixed-width rule, checked against the header rather than
    // taken on trust. It is what lets the callee refuse a struct_size it does
    // not know instead of reading past what the caller allocated.
    for s in structs_from_header() {
        let first: Vec<&str> = s.fields.iter().take(2).map(|f| f.name.as_str()).collect();
        assert_eq!(
            first,
            vec!["struct_size", "struct_version"],
            "`{}` does not open with struct_size and struct_version",
            s.name
        );
    }
}

#[test]
fn limits_layout_matches_the_header() {
    let rust = rust_struct!(
        ffi::viprs_acad_limits_v1,
        struct_size,
        struct_version,
        max_input_bytes,
        max_entities,
        max_string_bytes,
        max_polyline_points,
        max_block_depth,
        reserved0,
        max_output_bytes,
    );
    compare(&header_struct("viprs_acad_limits_v1"), &rust);
}

#[test]
fn capabilities_layout_matches_the_header() {
    let rust = rust_struct!(
        ffi::viprs_acad_capabilities_v1,
        struct_size,
        struct_version,
        abi_version,
        wire_version,
        dwg_version_min,
        dwg_version_max,
        supports_block_expansion,
        supports_warnings,
        reserved0,
        reserved1,
        reserved2,
    );
    compare(&header_struct("viprs_acad_capabilities_v1"), &rust);
}

#[test]
fn view_info_layout_matches_the_header() {
    let rust = rust_struct!(
        ffi::viprs_acad_view_info_v1,
        struct_size,
        struct_version,
        index,
        kind,
        min_x,
        min_y,
        max_x,
        max_y,
        entity_count,
    );
    compare(&header_struct("viprs_acad_view_info_v1"), &rust);
}
