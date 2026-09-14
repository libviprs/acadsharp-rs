//! Every bound the decoder enforces, set by the caller.
//!
//! Drawing files are untrusted input and the library is not a sandbox, so a
//! host makes its own tradeoff instead of inheriting one. The boundary reads a
//! zero field as "you pick", which is why every field here is an
//! [`Option<u64>`] and a fresh `Limits` is every field [`None`].
//!
//! What this deliberately does not do is write the documented defaults down.
//! 65536, 1000000 and 64 are the library's numbers, they move when the library
//! moves, and a Rust side copy of one goes on confidently reporting the old
//! bound after that happens. That is the same failure the fingerprint
//! handshake exists to catch, one layer up: agreement that outlived the thing
//! it was agreeing with. Ask the library, or leave the field alone.

/// The bounds a decode runs under.
///
/// Build one with [`Limits::new`] and the `with_*` setters:
///
/// ```
/// use acadsharp_rs::Limits;
///
/// let limits = Limits::new()
///     .with_max_polyline_points(100_000)
///     .with_max_input_bytes(64 * 1024 * 1024);
///
/// assert_eq!(limits.max_polyline_points(), Some(100_000));
/// // Everything else is still the library's to choose.
/// assert_eq!(limits.max_entities(), None);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Limits {
    max_input_bytes: Option<u64>,
    max_entities: Option<u64>,
    max_string_bytes: Option<u64>,
    max_polyline_points: Option<u64>,
    max_block_depth: Option<u64>,
    max_output_bytes: Option<u64>,
}

macro_rules! bound {
    ($field:ident, $with:ident, $what:expr) => {
        #[doc = $what]
        ///
        /// [`None`] leaves it to the library.
        #[must_use]
        pub const fn $field(&self) -> Option<u64> {
            self.$field
        }

        #[doc = concat!("Sets ", stringify!($field), ".")]
        ///
        #[doc = $what]
        ///
        /// A zero is the same as leaving it alone, because a zero field is
        /// what the boundary reads as "you pick".
        #[must_use]
        pub const fn $with(mut self, value: u64) -> Self {
            self.$field = Some(value);
            self
        }
    };
}

impl Limits {
    /// Every bound left to the library.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            max_input_bytes: None,
            max_entities: None,
            max_string_bytes: None,
            max_polyline_points: None,
            max_block_depth: None,
            max_output_bytes: None,
        }
    }

    bound!(
        max_input_bytes,
        with_max_input_bytes,
        "The largest input the open call will take, refused before the input is read."
    );
    bound!(
        max_entities,
        with_max_entities,
        "Entities counted across the whole decode, expansion of nested insertions included. It counts work and not only output, which is what stops a chain of block records expanding exponentially while emitting nothing."
    );
    bound!(
        max_string_bytes,
        with_max_string_bytes,
        "The longest UTF-8 string a text or warning record may carry. A warning message past it is shortened and marked; a text record and a view name are refused."
    );
    bound!(
        max_polyline_points,
        with_max_polyline_points,
        "Points in one record: a polyline's vertices, a polygon's, and a spline's control points and knots. Counted before the points are gathered."
    );
    bound!(
        max_block_depth,
        with_max_block_depth,
        "Nesting depth of an expansion, counting insertions, a dimension's picture and a hatch's boundary alike. The alternative to a bounded refusal here is a stack overflow."
    );
    bound!(
        max_output_bytes,
        with_max_output_bytes,
        "Total bytes the decode may emit across every batch."
    );
}
