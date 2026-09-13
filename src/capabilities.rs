//! What a build of the library can actually do, asked at run time.
//!
//! Asked rather than inferred from the version this crate compiled against.
//! The drawing-format range in particular comes from the backing reader and
//! moves when that does, which is exactly why a consumer should ask instead of
//! writing 1014 and 1032 into its own source. ABI.md says a consumer reads all
//! of this before it opens anything, so [`crate::Decoder::new`] reads it once
//! and every [`crate::Document`] carries a copy.

/// What the library behind this crate can do.
///
/// Accessors rather than public fields, because the C struct this comes from
/// is explicitly designed to grow: `struct_size` and `struct_version` exist so
/// a field can be added without breaking a consumer compiled against today's
/// header, and a public field here would turn that designed-non-breaking
/// change into a breaking one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capabilities {
    abi_version: u32,
    wire_version: u32,
    dwg_version_min: u32,
    dwg_version_max: u32,
    supports_block_expansion: bool,
    supports_warnings: bool,
    acadsharp_version: String,
}

impl Capabilities {
    pub(crate) fn from_raw(raw: crate::sys::RawCapabilities) -> Self {
        Self {
            abi_version: raw.abi_version,
            wire_version: raw.wire_version,
            dwg_version_min: raw.dwg_version_min,
            dwg_version_max: raw.dwg_version_max,
            // A flag is 0 or 1 and anything else is to be read as 1, never as
            // an error. No two languages agree on the width of a boolean, so
            // the boundary does not have one.
            supports_block_expansion: raw.supports_block_expansion != 0,
            supports_warnings: raw.supports_warnings != 0,
            acadsharp_version: raw.acadsharp_version,
        }
    }

    /// The `VIPRS_ACAD_ABI_VERSION` this build of the library reports.
    ///
    /// [`crate::Decoder::new`] has already compared it with
    /// [`crate::EXPECTED_ABI_VERSION`], so a `Decoder` that exists is one
    /// where these agree.
    #[must_use]
    pub const fn abi_version(&self) -> u32 {
        self.abi_version
    }

    /// The `VIPRS_ACAD_WIRE_VERSION` this build of the library writes.
    ///
    /// It moves on its own: a record's payload can gain a field without a
    /// single declaration in the header changing, so this and
    /// [`Capabilities::abi_version`] are read separately rather than either
    /// being inferred from the other.
    #[must_use]
    pub const fn wire_version(&self) -> u32 {
        self.wire_version
    }

    /// The oldest AC10xx drawing version this build reads, inclusive.
    #[must_use]
    pub const fn dwg_version_min(&self) -> u32 {
        self.dwg_version_min
    }

    /// The newest AC10xx drawing version this build reads, inclusive.
    #[must_use]
    pub const fn dwg_version_max(&self) -> u32 {
        self.dwg_version_max
    }

    /// Whether the decoder can flatten a nested insertion into transformed
    /// primitives. When it cannot, the stream still decodes: it just contains
    /// nothing from inside those insertions.
    #[must_use]
    pub const fn supports_block_expansion(&self) -> bool {
        self.supports_block_expansion
    }

    /// Whether reader notifications reach the stream as
    /// [`crate::Item::Warning`] items.
    #[must_use]
    pub const fn supports_warnings(&self) -> bool {
        self.supports_warnings
    }

    /// The pinned version of the backing reader, as UTF-8.
    #[must_use]
    pub fn acadsharp_version(&self) -> &str {
        &self.acadsharp_version
    }

    pub(crate) const fn dwg(&self) -> (u32, u32) {
        (self.dwg_version_min, self.dwg_version_max)
    }
}
