//! Regenerates and verifies `reference.json`, the hex-exact cross-SDK
//! test vectors for `pith-png`.
//!
//! Every vector is computed through the crate's public decode API from a
//! committed conformance fixture (`tests/fixtures/*.png`, byte-embedded
//! so the binary is hermetic): the decode output — `IHDR` facts plus the
//! concrete [`Pixels`] buffer — is serialized into a canonical byte
//! stream and hashed. No RNG, no time, no platform-dependent bytes, so
//! the output is byte-stable everywhere.
//!
//! Canonical decode-output serialization (the bytes `pixels_sha256`
//! covers):
//!
//! 1. `width` as `u32` big-endian, `height` as `u32` big-endian;
//! 2. `bit_depth`, `colour_type` (raw IHDR codes), `interlaced` as
//!    `0`/`1`, one byte each;
//! 3. a variant tag byte (`Gray8`=0 … `Rgba16`=5, declaration order);
//! 4. the pixel samples, row-major, channel-interleaved — `u8` verbatim,
//!    `u16` big-endian (PNG's own sample order).
//!
//! Usage:
//! - `gen-reference gen` — recompute every vector and write
//!   `reference.json` at the repository root.
//! - `gen-reference verify` — recompute and compare byte-for-byte
//!   against the committed copy; exit 1 on drift. This is the CI gate.

use std::fmt::Write as _;
use std::process::ExitCode;

use pith_digest::sha256;
use pith_png::reference::{canonical_bytes, variant_name};
use pith_png::{Limits, decode};

/// Where the committed copy lives, relative to the repository root.
const REFERENCE_PATH: &str = "reference.json";

/// One committed conformance fixture, embedded byte-exact (see
/// `tests/fixtures/PROVENANCE.md` for construction and digests).
struct Fixture {
    /// Vector name as it appears in `reference.json` (file stem).
    name: &'static str,
    /// The complete PNG stream.
    bytes: &'static [u8],
}

/// The 18 conformance fixtures, in `tests/fixtures/` order.
const FIXTURES: &[Fixture] = &[
    Fixture {
        name: "adam7_13x11_rgb",
        bytes: include_bytes!("../../tests/fixtures/adam7_13x11_rgb.png"),
    },
    Fixture {
        name: "adam7_2x2_gray8",
        bytes: include_bytes!("../../tests/fixtures/adam7_2x2_gray8.png"),
    },
    Fixture {
        name: "adam7_5x5_gray8",
        bytes: include_bytes!("../../tests/fixtures/adam7_5x5_gray8.png"),
    },
    Fixture {
        name: "ga16_2x2",
        bytes: include_bytes!("../../tests/fixtures/ga16_2x2.png"),
    },
    Fixture {
        name: "ga8_4x4",
        bytes: include_bytes!("../../tests/fixtures/ga8_4x4.png"),
    },
    Fixture {
        name: "gray16_3x2",
        bytes: include_bytes!("../../tests/fixtures/gray16_3x2.png"),
    },
    Fixture {
        name: "gray1_8x2",
        bytes: include_bytes!("../../tests/fixtures/gray1_8x2.png"),
    },
    Fixture {
        name: "gray4_5x3",
        bytes: include_bytes!("../../tests/fixtures/gray4_5x3.png"),
    },
    Fixture {
        name: "gray8_4x4",
        bytes: include_bytes!("../../tests/fixtures/gray8_4x4.png"),
    },
    Fixture {
        name: "gray8_trns_3x2",
        bytes: include_bytes!("../../tests/fixtures/gray8_trns_3x2.png"),
    },
    Fixture {
        name: "pal4_9x2",
        bytes: include_bytes!("../../tests/fixtures/pal4_9x2.png"),
    },
    Fixture {
        name: "pal8_trns_4x4",
        bytes: include_bytes!("../../tests/fixtures/pal8_trns_4x4.png"),
    },
    Fixture {
        name: "paeth_tie_2x2",
        bytes: include_bytes!("../../tests/fixtures/paeth_tie_2x2.png"),
    },
    Fixture {
        name: "rgb16_3x2",
        bytes: include_bytes!("../../tests/fixtures/rgb16_3x2.png"),
    },
    Fixture {
        name: "rgb8_filters_5x5",
        bytes: include_bytes!("../../tests/fixtures/rgb8_filters_5x5.png"),
    },
    Fixture {
        name: "rgb8_trns_3x2",
        bytes: include_bytes!("../../tests/fixtures/rgb8_trns_3x2.png"),
    },
    Fixture {
        name: "rgba16_2x2",
        bytes: include_bytes!("../../tests/fixtures/rgba16_2x2.png"),
    },
    Fixture {
        name: "rgba8_4x4",
        bytes: include_bytes!("../../tests/fixtures/rgba8_4x4.png"),
    },
];

// The canonical serialization (`variant_name`, `canonical_bytes`) lives
// in the library (`pith_png::reference`) so the generator and the C ABI
// surface share one implementation.

/// One decoded fixture: everything `reference.json` records per vector.
struct Measured {
    /// Image width in pixels.
    width: u32,
    /// Image height in pixels.
    height: u32,
    /// Bit depth the file was coded at.
    bit_depth: u8,
    /// Colour type code the file declared.
    colour_type: u8,
    /// Whether the file was Adam7-interlaced.
    interlaced: bool,
    /// The concrete `Pixels` variant the decode produced.
    pixels: &'static str,
    /// sha256 of the canonical decode-output serialization, hex.
    digest: String,
}

/// Decodes one fixture and measures everything `reference.json` records.
fn measure(f: &Fixture) -> Result<Measured, String> {
    let png = decode(f.bytes, &Limits::default())
        .map_err(|e| format!("{}: decode failed: {e}", f.name))?;
    let digest =
        sha256(&canonical_bytes(&png)).map_err(|e| format!("{}: sha256 failed: {e}", f.name))?;
    Ok(Measured {
        width: png.width(),
        height: png.height(),
        bit_depth: png.bit_depth(),
        colour_type: png.colour_type(),
        interlaced: png.interlaced(),
        pixels: variant_name(png.pixels()),
        digest: format!("{digest}"),
    })
}

/// Serializes the canonical `reference.json` bytes.
///
/// Deterministic: vectors sorted by name, two-space indentation, LF line
/// endings, a single trailing newline — byte-identical across
/// regenerations on every platform.
fn reference_json() -> String {
    let mut measured: Vec<(&str, Measured)> = FIXTURES
        .iter()
        .map(|f| (f.name, measure(f).expect("fixture must decode")))
        .collect();
    measured.sort_unstable_by_key(|(name, _)| *name);
    let mut out = String::from(
        "{\n  \"suite\": \"pith\",\n  \"crate\": \"pith-png\",\n  \"schema\": 1,\n  \"generator\": \"cargo run --bin gen-reference -- gen\",\n  \"vectors\": {\n",
    );
    for (i, (name, m)) in measured.iter().enumerate() {
        let _ = write!(
            out,
            "    \"{name}\": {{\"width\": {}, \"height\": {}, \"bit_depth\": {}, \
             \"colour_type\": {}, \"interlaced\": {}, \"pixels\": \"{}\", \
             \"pixels_sha256\": \"{}\"}}",
            m.width,
            m.height,
            m.bit_depth,
            m.colour_type,
            if m.interlaced { "true" } else { "false" },
            m.pixels,
            m.digest
        );
        out.push_str(if i + 1 == measured.len() { "\n" } else { ",\n" });
    }
    out.push_str("  }\n}\n");
    out
}

/// Compares the committed `reference.json` against a fresh regeneration,
/// returning a diff summary when they diverge.
fn verify() -> Result<(), String> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(REFERENCE_PATH);
    let committed = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    verify_str(&committed)
}

/// The comparison underlying [`verify`]: a committed render against a
/// fresh one.
fn verify_str(committed: &str) -> Result<(), String> {
    let fresh = reference_json();
    if committed == fresh {
        return Ok(());
    }
    Err(format!(
        "reference.json is stale: regenerate with `cargo run --bin gen-reference -- gen`\nexpected:\n{fresh}\ncommitted:\n{committed}"
    ))
}

fn run(arg: Option<&str>) -> ExitCode {
    match arg {
        Some("verify") => match verify() {
            Ok(()) => {
                println!("reference vectors are current");
                ExitCode::SUCCESS
            }
            Err(why) => {
                eprintln!("FAIL: {why}");
                ExitCode::FAILURE
            }
        },
        Some("gen") => {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(REFERENCE_PATH);
            std::fs::write(&path, reference_json())
                .unwrap_or_else(|e| panic!("cannot write {}: {e}", path.display()));
            println!("wrote {}", path.display());
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("usage: gen-reference gen|verify");
            ExitCode::from(2)
        }
    }
}

fn main() -> ExitCode {
    run(std::env::args().nth(1).as_deref())
}

#[cfg(test)]
mod tests {
    use super::{FIXTURES, canonical_bytes, measure, reference_json, run, verify_str};
    use pith_png::{Limits, decode};
    use std::process::ExitCode;

    /// Every fixture decodes and pins a distinct, well-formed digest.
    #[test]
    fn every_fixture_measures() {
        assert_eq!(
            FIXTURES.len(),
            18,
            "fixture drift: update PROVENANCE.md too"
        );
        let mut digests = Vec::new();
        for f in FIXTURES {
            let digest = measure(f).expect("fixture must decode").digest;
            assert_eq!(digest.len(), 64, "{}: digest is not sha256 hex", f.name);
            digests.push(digest);
        }
        digests.sort_unstable();
        digests.dedup();
        assert_eq!(digests.len(), FIXTURES.len(), "two fixtures hash alike");
    }

    /// The canonical serialization separates the six `Pixels` variants:
    /// same samples under a different variant must not collide.
    #[test]
    fn canonical_bytes_depend_on_variant_tag() {
        let bytes = decode(
            include_bytes!("../../tests/fixtures/gray8_4x4.png").as_slice(),
            &Limits::default(),
        )
        .expect("fixture decodes");
        assert!(matches!(bytes.pixels(), pith_png::Pixels::Gray8(_)));
        let canonical = canonical_bytes(&bytes);
        assert_eq!(canonical[11], 0, "Gray8 tag after the 11 header bytes");
        assert_eq!(
            &canonical[..11],
            b"\0\0\0\x04\0\0\0\x04\x08\0\0",
            "w=4 h=4 depth=8 ct=0 non-interlaced"
        );
        assert_eq!(
            canonical.len(),
            12 + 16,
            "11 header bytes + tag + 16 samples"
        );
    }

    /// The committed `reference.json` is current (CI gate, run as a test
    /// so plain `cargo test` catches drift without invoking the binary).
    #[test]
    fn committed_reference_is_current() {
        let committed =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/reference.json"))
                .expect("reference.json is committed at the repo root");
        verify_str(&committed).expect("committed reference.json is stale");
    }

    /// The committed copy round-trips through `verify`.
    #[test]
    fn verify_mode_accepts_fresh_render() {
        assert_eq!(verify_str(&reference_json()), Ok(()));
    }

    /// A drifted copy is rejected with the regeneration hint.
    #[test]
    fn verify_mode_rejects_drift() {
        let drifted = reference_json().replace("pith-png", "pith-XNG");
        assert_ne!(drifted, reference_json());
        assert!(verify_str(&drifted).is_err());
    }

    /// `verify` is the success path, `gen` writes the file, anything
    /// else is a usage error with exit code 2.
    #[test]
    fn run_modes() {
        assert_eq!(run(Some("verify")), ExitCode::SUCCESS);
        assert_eq!(run(Some("gen")), ExitCode::SUCCESS);
        assert_eq!(run(Some("bogus")), ExitCode::from(2));
        assert_eq!(run(None), ExitCode::from(2));
    }
}
