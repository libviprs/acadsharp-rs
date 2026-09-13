//! What comes out of a decode: owned primitives, warnings, and the records
//! that frame them.
//!
//! # Three dimensions, and the wire's own fields
//!
//! Every coordinate here is 3D, every record that has a normal carries it, and
//! a polyline's bulges cross one per vertex. Nothing is projected and nothing
//! is tessellated, because both of those need a tolerance or a plane and this
//! crate is not in a position to choose either. A bulge names an exact arc; a
//! run of segments does not, and a polyline of a thousand points does not tell
//! you it was a circle. `libviprs` picks a tolerance downstream, where the zoom
//! level and the output device are known.
//!
//! The two recipes a consumer needs for that are written down rather than
//! implemented. For the plane an arc's angles are measured in, from its own
//! normal, WIRE.md gives the arbitrary axis algorithm, including why the
//! `1/64` band is a real number and not an integer division. For subdividing a
//! bulge span it gives `b' = b / (1 + sqrt(1 + b²))` below `|b| = 1` and
//! `sign(b) / (r + sqrt(r² + 1))` with `r = 1 / |b|` above it, and the split at
//! 1 is load bearing rather than tidy.
//!
//! # Owned, and what that costs
//!
//! [`crate::PrimitiveStream`] refills one buffer per batch, so an item that
//! borrowed it could not outlive the next call. These are owned, which means a
//! polyline's vertices are copied out of the batch once. [`crate::batch`] is
//! the zero-copy layer underneath and stays available for a caller who wants
//! to walk bytes they already hold.

//! # These structs grow fields, and say so
//!
//! Every payload struct here is `#[non_exhaustive]`. The wire has already done
//! this once: WIRE.md says version 2's `Polyline` is thirty two bytes longer
//! than version 1's. So field growth is what this format's evolution looks
//! like, it is designed to be non-breaking, and an exhaustive destructure from
//! outside this crate would turn it into a breaking release. The crate makes
//! exactly this argument for [`crate::Capabilities`], which answers it with
//! accessors; here the fields stay public and read-only construction is what
//! moves.
//!
//! Nothing in this crate hands out a half-built one, so the cost lands on a
//! consumer writing their own test. [`Default`] is the answer for the seven
//! geometry structs and [`Warning::new`] for the eighth: build one, set the
//! fields you care about, and a field added later leaves your test compiling.

use core::fmt;

use crate::View;
use crate::batch::{self, Record, record_type};

/// The backing file's handle for whatever produced a record.
///
/// A newtype rather than a bare `u64` so it cannot be swapped with a count, an
/// index or a length by accident, all of which are `u64` on this boundary too.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ItemHandle(u64);

impl ItemHandle {
    /// Wraps a raw handle.
    #[must_use]
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    /// The raw handle.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// `None` for zero, which is how the wire spells "no item".
    const fn from_wire(raw: u64) -> Option<Self> {
        if raw == 0 { None } else { Some(Self(raw)) }
    }
}

impl fmt::Display for ItemHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Where a primitive came from in the drawing.
///
/// One struct rather than the same two fields repeated on eight types, which
/// is also how the sixteen byte geometry prologue is shaped on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct Origin {
    /// The entity that produced this record, if the file had a handle for it.
    pub item_handle: Option<ItemHandle>,
    /// Set when this record came from expanding a nested insertion, so the
    /// coordinates have already been transformed into the containing space.
    pub from_expanded_insert: bool,
}

impl From<batch::Prologue> for Origin {
    fn from(prologue: batch::Prologue) -> Self {
        Self {
            item_handle: ItemHandle::from_wire(prologue.item_handle),
            from_expanded_insert: prologue.from_expanded_insert(),
        }
    }
}

/// Two endpoints.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[non_exhaustive]
pub struct Line {
    /// Where this came from.
    pub origin: Origin,
    /// The first endpoint.
    pub start: [f64; 3],
    /// The second endpoint.
    pub end: [f64; 3],
}

/// A vertex run, open or closed, with a normal and a bulge per span.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct Polyline {
    /// Where this came from.
    pub origin: Origin,
    /// Whether the last vertex joins back to the first.
    pub closed: bool,
    /// The entity's normal, a unit vector.
    pub normal: [f64; 3],
    /// The vertices, in order.
    pub vertices: Vec<[f64; 3]>,
    /// One bulge per vertex, or empty when every span is straight.
    ///
    /// `bulges[i]` belongs to the span from vertex `i` to vertex `i + 1`, and
    /// on a closed polyline `bulges[n - 1]` is the closing span's. A bulge is
    /// `tan(θ / 4)` for the arc's included angle, positive counter-clockwise
    /// about [`Polyline::normal`], and zero is a straight span.
    pub bulges: Vec<f64>,
}

/// Centre, radius, and a start and end angle in the plane the normal defines.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[non_exhaustive]
pub struct Arc {
    /// Where this came from.
    pub origin: Origin,
    /// The centre.
    pub centre: [f64; 3],
    /// The radius.
    pub radius: f64,
    /// The start angle in radians, counter-clockwise about the normal.
    pub start_angle: f64,
    /// The end angle in radians.
    pub end_angle: f64,
    /// The entity's normal, a unit vector, which is what decides where angle
    /// zero points. WIRE.md's arbitrary axis algorithm is the recipe.
    pub normal: [f64; 3],
}

/// Centre, radius, and a normal.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[non_exhaustive]
pub struct Circle {
    /// Where this came from.
    pub origin: Origin,
    /// The centre.
    pub centre: [f64; 3],
    /// The radius.
    pub radius: f64,
    /// The entity's normal, a unit vector.
    pub normal: [f64; 3],
}

/// Centre, major axis, minor-to-major ratio and a parameter range.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[non_exhaustive]
pub struct Ellipse {
    /// Where this came from.
    pub origin: Origin,
    /// The centre.
    pub centre: [f64; 3],
    /// The vector from the centre to the end of the major axis.
    pub major_axis: [f64; 3],
    /// Minor over major.
    pub ratio: f64,
    /// The start of the parameter range.
    pub start_param: f64,
    /// The end of the parameter range.
    pub end_param: f64,
    /// The entity's normal, a unit vector.
    pub normal: [f64; 3],
}

/// Degree, knots, control points and weights, untessellated.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct Spline {
    /// Where this came from.
    pub origin: Origin,
    /// The degree.
    pub degree: u32,
    /// The flag word. [`Spline::CLOSED`], [`Spline::RATIONAL`] and
    /// [`Spline::PERIODIC`] name the bits this wire version defines; the rest
    /// are carried through rather than refused.
    pub flags: u32,
    /// The knot vector.
    pub knots: Vec<f64>,
    /// The control points.
    pub controls: Vec<[f64; 3]>,
    /// One weight per control point, or empty when the spline is not rational.
    pub weights: Vec<f64>,
}

impl Spline {
    /// Bit 0 of [`Spline::flags`].
    pub const CLOSED: u32 = 1;
    /// Bit 1 of [`Spline::flags`].
    pub const RATIONAL: u32 = 2;
    /// Bit 2 of [`Spline::flags`].
    pub const PERIODIC: u32 = 4;

    /// Whether [`Spline::CLOSED`] is set.
    #[must_use]
    pub const fn is_closed(&self) -> bool {
        self.flags & Self::CLOSED != 0
    }

    /// Whether [`Spline::RATIONAL`] is set.
    #[must_use]
    pub const fn is_rational(&self) -> bool {
        self.flags & Self::RATIONAL != 0
    }

    /// Whether [`Spline::PERIODIC`] is set.
    #[must_use]
    pub const fn is_periodic(&self) -> bool {
        self.flags & Self::PERIODIC != 0
    }
}

/// A position, a height, a rotation and the drawing's own text.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct Text {
    /// Where this came from.
    pub origin: Origin,
    /// The insertion point.
    pub position: [f64; 3],
    /// The height, in drawing units.
    pub height: f64,
    /// The rotation, in radians.
    pub rotation: f64,
    /// The text, never shortened: a consumer has no way to tell a label the
    /// file carries from one a producer cut, so a string past
    /// [`crate::Limits::with_max_string_bytes`] is refused instead.
    pub text: String,
}

/// One shape out of a drawing.
///
/// `#[non_exhaustive]`, so a later wire version adding a record type is not a
/// breaking change here. Adding a variant is a change to this crate rather
/// than to the wire, and that is exactly why the attribute is on it.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Primitive {
    /// Wire type 3.
    Line(Line),
    /// Wire type 4.
    Polyline(Polyline),
    /// Wire type 5.
    Arc(Arc),
    /// Wire type 6.
    Circle(Circle),
    /// Wire type 7.
    Ellipse(Ellipse),
    /// Wire type 8.
    Spline(Spline),
    /// Wire type 9: a closed boundary, carrying a polyline's payload with
    /// `closed` set. The first vertex is not repeated, so the closing span is
    /// the one the last bulge describes.
    Polygon(Polyline),
    /// Wire type 10.
    Text(Text),
}

impl Primitive {
    /// The wire number this primitive came from.
    #[must_use]
    pub const fn record_type(&self) -> u16 {
        match self {
            Self::Line(_) => record_type::LINE,
            Self::Polyline(_) => record_type::POLYLINE,
            Self::Arc(_) => record_type::ARC,
            Self::Circle(_) => record_type::CIRCLE,
            Self::Ellipse(_) => record_type::ELLIPSE,
            Self::Spline(_) => record_type::SPLINE,
            Self::Polygon(_) => record_type::POLYGON,
            Self::Text(_) => record_type::TEXT,
        }
    }

    /// Where this primitive came from in the drawing.
    #[must_use]
    pub const fn origin(&self) -> Origin {
        match self {
            Self::Line(l) => l.origin,
            Self::Polyline(p) | Self::Polygon(p) => p.origin,
            Self::Arc(a) => a.origin,
            Self::Circle(c) => c.origin,
            Self::Ellipse(e) => e.origin,
            Self::Spline(s) => s.origin,
            Self::Text(t) => t.origin,
        }
    }
}

/// A warning code.
///
/// A newtype over the number and never a closed enum. Codes from 1000 up are
/// allocated by whatever read the drawing and are not part of the VIPRS
/// specification at all, so a consumer is entitled to know none of them: the
/// library's own synthetic document emits 1100. A closed set would break on
/// the first real stream, and adding a code is deliberately not a wire version
/// bump, so a consumer that refused an unknown one would start failing on
/// files it read correctly the day before.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WarningCode(u32);

impl WarningCode {
    /// An entity kind this build does not flatten. The message names the
    /// source format's type.
    pub const UNSUPPORTED_ENTITY: Self = Self(100);
    /// Something the backing reader had to say about the file.
    pub const READER_NOTIFICATION: Self = Self(101);
    /// A dimension with no geometry block to take its lines and text from.
    pub const DIMENSION_WITHOUT_BLOCK: Self = Self(102);
    /// A hatch with no boundary loop that could become a polygon.
    pub const HATCH_PATTERN_ONLY: Self = Self(103);
    /// A boundary loop carrying an elliptical or spline edge. The edges follow
    /// as their own records, so nothing is lost and nothing is approximated.
    pub const HATCH_LOOP_NOT_POLYGON: Self = Self(104);
    /// An insertion whose block could not be resolved, which is what an
    /// unresolved external reference looks like from inside.
    pub const UNRESOLVED_BLOCK: Self = Self(105);
    /// A block transform that does not scale an entity's plane uniformly. The
    /// parameters still cross unchanged.
    pub const NON_UNIFORM_BLOCK_SCALE: Self = Self(106);
    /// A geometry record whose values were not all finite, so it was not
    /// emitted: there is no correct number to put in its place.
    pub const NON_FINITE_GEOMETRY: Self = Self(107);
    /// This view emitted no geometry record at all.
    ///
    /// Read it with what sits beside it. This alone, with the inverted extents
    /// that make [`crate::View::extents`] `None`, is a drawing with nothing in
    /// it; the same pair with other warnings around it is a drawing something
    /// went wrong reading.
    pub const EMPTY_VIEW: Self = Self(108);

    /// The lowest code the backing source allocates.
    pub const BACKEND_RANGE_START: u32 = 1000;

    /// Wraps a raw code.
    #[must_use]
    pub const fn new(raw: u32) -> Self {
        Self(raw)
    }

    /// The raw code.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }

    /// Whether this code belongs to the backing source rather than to VIPRS.
    #[must_use]
    pub const fn is_backend_allocated(self) -> bool {
        self.0 >= Self::BACKEND_RANGE_START
    }
}

impl fmt::Display for WarningCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match *self {
            Self::UNSUPPORTED_ENTITY => "unsupported entity",
            Self::READER_NOTIFICATION => "reader notification",
            Self::DIMENSION_WITHOUT_BLOCK => "dimension without block",
            Self::HATCH_PATTERN_ONLY => "hatch pattern only",
            Self::HATCH_LOOP_NOT_POLYGON => "hatch loop not a polygon",
            Self::UNRESOLVED_BLOCK => "unresolved block",
            Self::NON_UNIFORM_BLOCK_SCALE => "non-uniform block scale",
            Self::NON_FINITE_GEOMETRY => "non-finite geometry",
            Self::EMPTY_VIEW => "empty view",
            other if other.is_backend_allocated() => "a code the backing source allocated",
            _ => "a code this build has no name for",
        };
        write!(f, "{} ({})", name, self.0)
    }
}

/// Something the decoder had to say about part of the drawing.
///
/// Warnings are not errors. A decode that emits a hundred of them and finishes
/// succeeded, and they are what it has to say about the parts it could not
/// fully represent. They arrive inline in the stream rather than through a
/// side channel, because reading one often means reading what sits beside it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Warning {
    /// What kind of warning this is.
    pub code: WarningCode,
    /// For a person reading a log. Nothing branches on it, and a message past
    /// [`crate::Limits::with_max_string_bytes`] is shortened and marked with a
    /// trailing ` [truncated]` rather than refused.
    pub message: String,
    /// The entity this is about, or `None` when it is about the document or
    /// the view.
    pub entity: Option<ItemHandle>,
}

impl Warning {
    /// A warning about the document or the view.
    ///
    /// `entity` is a public field, so set it afterwards for one about a
    /// specific entity. This exists because [`Warning`] is
    /// `#[non_exhaustive]` and [`WarningCode`] has no sensible default, so
    /// there is otherwise no way for a consumer's own test to produce one.
    ///
    /// ```
    /// use acadsharp_rs::{ItemHandle, Warning, WarningCode};
    ///
    /// let mut warning = Warning::new(WarningCode::EMPTY_VIEW, "nothing in it");
    /// warning.entity = Some(ItemHandle::new(42));
    /// assert_eq!(warning.code, WarningCode::EMPTY_VIEW);
    /// ```
    #[must_use]
    pub fn new(code: WarningCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            entity: None,
        }
    }
}

/// One thing out of the decode stream.
///
/// The frame records are here as well as the geometry, because
/// [`crate::batch::ViewEnd`] and [`crate::batch::DocumentEnd`] carry the
/// totals, and those totals are the only proof a caller gets that a stream was
/// not truncated. [`crate::PrimitiveStream`] also remembers them, so a caller
/// who only wants shapes can ignore these variants and still ask afterwards.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Item {
    /// Wire type 1, which opens the stream.
    DocumentBegin(batch::DocumentBegin),
    /// Wire type 2, which opens the one view this decode covers.
    ViewBegin(View),
    /// A shape.
    Primitive(Primitive),
    /// Something the decoder had to say.
    Warning(Warning),
    /// Wire type 12, carrying how many records the view held.
    ViewEnd(batch::ViewEnd),
    /// Wire type 13, carrying the totals for the whole stream.
    DocumentEnd(batch::DocumentEnd),
    /// A record type this build does not know, skipped by its length and
    /// carried through so it can be logged.
    ///
    /// Types from 32512 up are the forward-probe range and the library emits
    /// one on purpose, so this arm runs against a real stream on every decode
    /// rather than only against a buffer a test assembled.
    Unknown {
        /// The wire number.
        record_type: u16,
        /// The payload, verbatim and uninterpreted.
        payload: Vec<u8>,
    },
}

impl Item {
    /// The wire number this item came from.
    #[must_use]
    pub const fn record_type(&self) -> u16 {
        match self {
            Self::DocumentBegin(_) => record_type::DOCUMENT_BEGIN,
            Self::ViewBegin(_) => record_type::VIEW_BEGIN,
            Self::Primitive(primitive) => primitive.record_type(),
            Self::Warning(_) => record_type::WARNING,
            Self::ViewEnd(_) => record_type::VIEW_END,
            Self::DocumentEnd(_) => record_type::DOCUMENT_END,
            Self::Unknown { record_type, .. } => *record_type,
        }
    }

    /// The shape, if this item is one.
    #[must_use]
    pub const fn primitive(&self) -> Option<&Primitive> {
        match self {
            Self::Primitive(primitive) => Some(primitive),
            _ => None,
        }
    }

    /// Copies one borrowed record out of a batch buffer.
    ///
    /// This is the only place anything is copied. The buffer underneath is
    /// reused for the next batch, so an item that borrowed it could not
    /// outlive the call that refills it, and making that a copy at a named
    /// point beats making it a lifetime a caller has to thread through their
    /// own code.
    pub(crate) fn from_record(record: &Record<'_>) -> Self {
        match record {
            Record::DocumentBegin(d) => Self::DocumentBegin(*d),
            Record::ViewBegin(v) => Self::ViewBegin(View::from_record(v)),
            Record::Line(l) => Self::Primitive(Primitive::Line(Line {
                origin: l.prologue.into(),
                start: l.start,
                end: l.end,
            })),
            Record::Polyline(p) => Self::Primitive(Primitive::Polyline(polyline(p))),
            Record::Polygon(p) => Self::Primitive(Primitive::Polygon(polyline(p))),
            Record::Arc(a) => Self::Primitive(Primitive::Arc(Arc {
                origin: a.prologue.into(),
                centre: a.centre,
                radius: a.radius,
                start_angle: a.start_angle,
                end_angle: a.end_angle,
                normal: a.normal,
            })),
            Record::Circle(c) => Self::Primitive(Primitive::Circle(Circle {
                origin: c.prologue.into(),
                centre: c.centre,
                radius: c.radius,
                normal: c.normal,
            })),
            Record::Ellipse(e) => Self::Primitive(Primitive::Ellipse(Ellipse {
                origin: e.prologue.into(),
                centre: e.centre,
                major_axis: e.major_axis,
                ratio: e.ratio,
                start_param: e.start_param,
                end_param: e.end_param,
                normal: e.normal,
            })),
            Record::Spline(s) => Self::Primitive(Primitive::Spline(Spline {
                origin: s.prologue.into(),
                degree: s.degree,
                flags: s.flags,
                knots: s.knots.iter().collect(),
                controls: s.controls.iter().collect(),
                weights: s.weights.iter().collect(),
            })),
            Record::Text(t) => Self::Primitive(Primitive::Text(Text {
                origin: t.prologue.into(),
                position: t.position,
                height: t.height,
                rotation: t.rotation,
                text: t.text.to_owned(),
            })),
            Record::Warning(w) => Self::Warning(Warning {
                code: WarningCode::new(w.code),
                message: w.message.to_owned(),
                entity: ItemHandle::from_wire(w.item_handle),
            }),
            Record::ViewEnd(v) => Self::ViewEnd(*v),
            Record::DocumentEnd(d) => Self::DocumentEnd(*d),
            Record::Unknown(u) => Self::Unknown {
                record_type: u.record_type,
                payload: u.payload.to_vec(),
            },
            // No catch-all arm. `Record` is `#[non_exhaustive]` for the
            // outside world, and inside this crate that attribute does
            // nothing, so a record type the batch decoder gains lands here as
            // a compile error naming this match. A `_` arm would have turned
            // that into a new wire record quietly arriving as `Unknown`, which
            // is the one failure mode nobody would notice.
        }
    }
}

fn polyline(p: &batch::Polyline<'_>) -> Polyline {
    Polyline {
        origin: p.prologue.into(),
        closed: p.closed,
        normal: p.normal,
        vertices: p.vertices.iter().collect(),
        bulges: p.bulges.iter().collect(),
    }
}
