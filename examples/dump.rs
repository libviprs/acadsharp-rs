//! Dumps one view of one drawing in the text form libviprs-dep recorded.
//!
//! ```text
//! cargo run --example dump -- <file.dwg> [<view-index>]
//! ```
//!
//! The output is `CanonicalDump.cs`'s format from libviprs-dep's
//! `acadsharp/tests/fixtures/gen/`, byte for byte: `NNNNN Kind handle=HEX
//! flags=N ...` with six-decimal numbers, `-0.000000` collapsed to `0.000000`,
//! the same string escaping, and the warning code spelled the way
//! `WarningCodes.Name` spells it. The document and view framing records are
//! left out, as the recording leaves them out.
//!
//! Why this exists: that recording was made by the JIT fixture generator over
//! the same flattener sources the NativeAOT shim compiles. A dump of this
//! crate in the same format, diffed against the recording, therefore isolates
//! JIT against AOT (trimming, globalization, the wire's encoding) with the
//! flattener held constant. The verdict is a line diff of this output against
//! `expectations/<id>.txt`, and the first differing line is the first
//! differing record. `tools/aot_parity.sh` does that over a directory.
//!
//! An example gets this crate's own rpath, so no build script is needed. In a
//! build with no native archive the first call answers `Error::Unlinked`, and
//! this exits 2 with the message, never 0.
//!
//! One thing it cannot reproduce: `flags=` is printed from the one flag the
//! safe API exposes, "expanded from a nested insertion" (bit 0). The
//! recordings carry only 0 and 1 there, so nothing is lost today, but a
//! future wire bit would not show.

use std::fmt::Write as _;
use std::io::Write as _;
use std::process::ExitCode;

use acadsharp_rs::item::{Polyline, Warning};
use acadsharp_rs::{Decoder, Document, Item, Limits, Primitive};

/// Wire types from this number up are the forward probe, which the recording
/// treats as framing.
const FORWARD_PROBE_FIRST: u16 = 32512;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: dump <file.dwg> [<view-index>]");
        return ExitCode::from(64);
    };
    let view: u32 = match args.next() {
        None => 0,
        Some(v) => match v.parse() {
            Ok(v) => v,
            Err(_) => {
                eprintln!("dump: view index must be a number, got {v:?}");
                return ExitCode::from(64);
            }
        },
    };
    if args.next().is_some() {
        eprintln!("usage: dump <file.dwg> [<view-index>]");
        return ExitCode::from(64);
    }

    match run(&path, view) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("dump: {message}");
            ExitCode::from(2)
        }
    }
}

fn run(path: &str, view: u32) -> Result<(), String> {
    let decoder = Decoder::new().map_err(|e| e.to_string())?;
    let document = Document::open_path(&decoder, path, &Limits::default())
        .map_err(|e| format!("could not open {path}: {e}"))?;
    let stream = document
        .decode(view)
        .map_err(|e| format!("could not decode view {view} of {path}: {e}"))?;

    // Everything is collected before anything is printed, so a decode that
    // fails half way prints nothing and exits non-zero rather than leaving a
    // truncated dump that a diff could mistake for a short recording.
    let mut out = String::new();
    let mut index = 0usize;
    for item in stream {
        let item = item.map_err(|e| format!("decode of {path} failed after {index} records: {e}"))?;
        let Some(line) = line(index, &item)? else {
            continue;
        };
        out.push_str(&line);
        out.push('\n');
        index += 1;
    }

    let stdout = std::io::stdout();
    let mut lock = stdout.lock();
    lock.write_all(out.as_bytes())
        .and_then(|()| lock.flush())
        .map_err(|e| format!("could not write the dump: {e}"))
}

/// One record's line, or `None` for the framing the recording leaves out.
fn line(index: usize, item: &Item) -> Result<Option<String>, String> {
    let mut s = String::new();
    match item {
        Item::DocumentBegin(_) | Item::ViewBegin(_) | Item::ViewEnd(_) | Item::DocumentEnd(_) => {
            return Ok(None);
        }
        Item::Unknown { record_type, .. } if *record_type >= FORWARD_PROBE_FIRST => {
            return Ok(None);
        }
        Item::Unknown { record_type, .. } => {
            return Err(format!(
                "record {index} is wire type {record_type}, which the recording would print as \
                 Type{record_type} with a handle this API does not expose"
            ));
        }
        Item::Warning(w) => warning(&mut s, index, w),
        Item::Primitive(p) => primitive(&mut s, index, p),
        // `Item` is non-exhaustive; a variant added later must be looked at
        // rather than silently dropped from a parity measurement.
        _ => return Err(format!("record {index} is an item this example does not know")),
    }
    Ok(Some(s))
}

fn head(s: &mut String, index: usize, kind: &str, handle: u64, flags: u32) {
    let _ = write!(s, "{index:05} {kind} handle={handle:X} flags={flags}");
}

fn warning(s: &mut String, index: usize, w: &Warning) {
    head(s, index, "Warning", w.entity.map_or(0, |h| h.get()), 0);
    let _ = write!(
        s,
        " code={} message={}",
        warning_name(w.code.get()),
        quote(&w.message)
    );
}

/// `WarningCodes.Name`, including its `WARNING_<n>` fallback.
fn warning_name(code: u32) -> String {
    match code {
        100 => "UNSUPPORTED_ENTITY",
        101 => "READER_NOTIFICATION",
        102 => "DIMENSION_WITHOUT_BLOCK",
        103 => "HATCH_PATTERN_ONLY",
        104 => "HATCH_LOOP_NOT_POLYGON",
        105 => "UNRESOLVED_BLOCK",
        106 => "NON_UNIFORM_BLOCK_SCALE",
        107 => "NON_FINITE_GEOMETRY",
        108 => "EMPTY_VIEW",
        109 => "ENTITY_REFUSED_BY_DESIGN",
        110 => "MESH_SUBDIVISION_IGNORED",
        111 => "MESH_FACE_UNREADABLE",
        112 => "POLYGON_MASKS",
        113 => "FACE_EDGE_VISIBILITY_IGNORED",
        114 => "ARROWHEAD_NOT_DRAWN",
        115 => "MLINE_STYLE_FEATURES_IGNORED",
        other => return format!("WARNING_{other}"),
    }
    .to_string()
}

fn primitive(s: &mut String, index: usize, p: &Primitive) {
    let origin = p.origin();
    let handle = origin.item_handle.map_or(0, |h| h.get());
    let flags = u32::from(origin.from_expanded_insert);
    match p {
        Primitive::Line(l) => {
            head(s, index, "Line", handle, flags);
            triples(s, "pts", &[l.start, l.end]);
        }
        Primitive::Arc(a) => {
            head(s, index, "Arc", handle, flags);
            triples(s, "c", &[a.centre]);
            let _ = write!(
                s,
                " r={} a0={} a1={}",
                n(a.radius),
                n(a.start_angle),
                n(a.end_angle)
            );
            triples(s, "normal", &[a.normal]);
        }
        Primitive::Circle(c) => {
            head(s, index, "Circle", handle, flags);
            triples(s, "c", &[c.centre]);
            let _ = write!(s, " r={}", n(c.radius));
            triples(s, "normal", &[c.normal]);
        }
        Primitive::Ellipse(e) => {
            head(s, index, "Ellipse", handle, flags);
            triples(s, "c", &[e.centre]);
            triples(s, "major", &[e.major_axis]);
            let _ = write!(
                s,
                " ratio={} p0={} p1={}",
                n(e.ratio),
                n(e.start_param),
                n(e.end_param)
            );
            triples(s, "normal", &[e.normal]);
        }
        Primitive::Polyline(pl) => polyline(s, index, "Polyline", handle, flags, pl),
        Primitive::Polygon(pl) => polyline(s, index, "Polygon", handle, flags, pl),
        Primitive::Spline(sp) => {
            head(s, index, "Spline", handle, flags);
            let _ = write!(s, " degree={} splineflags={}", sp.degree, sp.flags);
            values(s, "knots", &sp.knots);
            triples(s, "ctrl", &sp.controls);
            values(s, "weights", &sp.weights);
        }
        Primitive::Text(t) => {
            head(s, index, "Text", handle, flags);
            triples(s, "p", &[t.position]);
            let _ = write!(s, " h={} rot={}", n(t.height), n(t.rotation));
            let _ = write!(s, " value={}", quote(&t.text));
        }
        // Non-exhaustive: see `line`. Unreachable for the wire this crate
        // decodes today, and loud if it ever is not.
        _ => {
            head(s, index, "Unknown", handle, flags);
        }
    }
}

fn polyline(s: &mut String, index: usize, kind: &str, handle: u64, flags: u32, p: &Polyline) {
    head(s, index, kind, handle, flags);
    let _ = write!(s, " n={} closed={}", p.vertices.len(), u32::from(p.closed));
    triples(s, "pts", &p.vertices);
    values(s, "bulges", &p.bulges);
    triples(s, "normal", &[p.normal]);
}

fn values(s: &mut String, name: &str, v: &[f64]) {
    let _ = write!(s, " {name}=[");
    for (i, x) in v.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&n(*x));
    }
    s.push(']');
}

fn triples(s: &mut String, name: &str, v: &[[f64; 3]]) {
    let _ = write!(s, " {name}=[");
    for (i, t) in v.iter().enumerate() {
        if i > 0 {
            s.push(';');
        }
        let _ = write!(s, "({},{},{})", n(t[0]), n(t[1]), n(t[2]));
    }
    s.push(']');
}

/// `CanonicalDump.Q`.
fn quote(text: &str) -> String {
    let mut s = String::from("\"");
    for ch in text.chars() {
        match ch {
            '"' => s.push_str("\\\""),
            '\\' => s.push_str("\\\\"),
            '\n' => s.push_str("\\n"),
            '\r' => s.push_str("\\r"),
            '\t' => s.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(s, "\\u{:04x}", c as u32);
            }
            c => s.push(c),
        }
    }
    s.push('"');
    s
}

/// `CanonicalDump.N`: .NET's `ToString("0.000000", InvariantCulture)` with
/// `-0.000000` collapsed.
///
/// A custom numeric format on a double does not print the exact value. .NET
/// first renders 15 significant digits and then rounds that digit string half
/// up (away from zero), so a value like 0.0078125, which is exactly a tie at
/// the sixth decimal, prints `0.007813`, where Rust's `{:.6}` would round the
/// exact value half to even and print `0.007812`. This does the same two
/// steps rather than hoping no recorded value is one of those.
fn n(x: f64) -> String {
    if x.is_nan() {
        return "NaN".to_string();
    }
    if x.is_infinite() {
        return if x < 0.0 { "-Infinity" } else { "Infinity" }.to_string();
    }
    if x == 0.0 {
        return "0.000000".to_string();
    }

    // 15 significant digits, as "d.dddddddddddddde<exp>".
    let sci = format!("{:.14e}", x.abs());
    let (mantissa, exp) = sci.split_once('e').expect("scientific notation");
    let exp: i32 = exp.parse().expect("exponent");
    let mut digits: Vec<u8> = mantissa
        .bytes()
        .filter(u8::is_ascii_digit)
        .map(|b| b - b'0')
        .collect();
    // The value is 0.d1d2d3... times 10^point.
    let point = exp + 1;

    // Keep `point + 6` digits: everything left of the point and six decimals.
    let keep = point + 6;
    if keep < 0 {
        return "0.000000".to_string();
    }
    let keep = keep as usize;
    let round_up = digits.get(keep).is_some_and(|d| *d >= 5);
    digits.truncate(keep);
    while digits.len() < keep {
        digits.push(0);
    }
    if round_up {
        let mut i = digits.len();
        loop {
            if i == 0 {
                digits.insert(0, 1);
                break;
            }
            i -= 1;
            if digits[i] == 9 {
                digits[i] = 0;
            } else {
                digits[i] += 1;
                break;
            }
        }
    }

    // `digits` now holds an integer scaled by 10^6. Pad so there are at least
    // seven digits, then put the point six from the end.
    while digits.len() < 7 {
        digits.insert(0, 0);
    }
    let split = digits.len() - 6;
    let mut whole: String = digits[..split].iter().map(|d| char::from(b'0' + d)).collect();
    // Leading zeros beyond the first can appear after padding; trim them.
    while whole.len() > 1 && whole.starts_with('0') {
        whole.remove(0);
    }
    let frac: String = digits[split..].iter().map(|d| char::from(b'0' + d)).collect();

    let zero = whole == "0" && frac.bytes().all(|b| b == b'0');
    if x < 0.0 && !zero {
        format!("-{whole}.{frac}")
    } else {
        format!("{whole}.{frac}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_print_the_way_the_recording_prints_them() {
        assert_eq!(n(0.0), "0.000000");
        assert_eq!(n(-0.0), "0.000000");
        assert_eq!(n(1.5), "1.500000");
        assert_eq!(n(-2.5), "-2.500000");
        assert_eq!(n(41.5), "41.500000");
        assert_eq!(n(0.25), "0.250000");
        assert_eq!(n(920.679627), "920.679627");
        assert_eq!(n(100.0), "100.000000");
        assert_eq!(n(9.9999996), "10.000000");
        assert_eq!(n(0.999_999_6), "1.000000");
        assert_eq!(n(1e20), "100000000000000000000.000000");
    }

    #[test]
    fn a_negative_that_rounds_to_zero_loses_its_sign() {
        assert_eq!(n(-0.000_000_4), "0.000000");
        assert_eq!(n(-1e-12), "0.000000");
    }

    #[test]
    fn a_tie_at_the_sixth_decimal_rounds_away_from_zero() {
        // 0.0078125 is exactly 2^-7, so it is a true tie. .NET prints
        // 0.007813 and a plain `{:.6}` prints 0.007812.
        assert_eq!(n(0.007_812_5), "0.007813");
        assert_eq!(n(-0.007_812_5), "-0.007813");
    }

    #[test]
    fn digits_past_the_fifteenth_are_not_printed() {
        // .NET renders 15 significant digits first, so a large value ends in
        // zeros rather than in the exact expansion.
        assert_eq!(n(123_456_789_012_345_678.0), "123456789012346000.000000");
    }

    #[test]
    fn non_finite_numbers_print_the_way_dotnet_spells_them() {
        assert_eq!(n(f64::NAN), "NaN");
        assert_eq!(n(f64::INFINITY), "Infinity");
        assert_eq!(n(f64::NEG_INFINITY), "-Infinity");
    }

    #[test]
    fn strings_are_escaped_the_way_q_escapes_them() {
        assert_eq!(quote("a\"b\\c\nd\re\tf"), "\"a\\\"b\\\\c\\nd\\re\\tf\"");
        assert_eq!(quote("\u{1}"), "\"\\u0001\"");
        assert_eq!(quote("ÄÖÜ"), "\"ÄÖÜ\"");
    }

    #[test]
    fn warning_codes_are_named_and_unknown_ones_fall_back() {
        assert_eq!(warning_name(101), "READER_NOTIFICATION");
        assert_eq!(warning_name(115), "MLINE_STYLE_FEATURES_IGNORED");
        assert_eq!(warning_name(1100), "WARNING_1100");
    }
}
