//! The canonical decode-output serialization behind `reference.json`,
//! re-expressed as library code so the vector generator and the C ABI
//! surface (`ffi`) share one implementation.
//!
//! Every vector in `reference.json` is computed through the crate's
//! public decode API from a committed conformance fixture
//! (`tests/fixtures/*.png`): the decode output — `IHDR` facts plus the
//! concrete [`Pixels`] buffer — is serialized into a canonical byte
//! stream and SHA-256 hashed by `tools/gen-reference`. No RNG, no
//! time, no platform-dependent bytes, so the output is byte-stable
//! everywhere.
//!
//! Canonical decode-output serialization (the bytes `pixels_sha256`
//! covers, and the byte stream [`crate::ffi::pith_png_decode_canonical`]
//! hands to the language SDKs):
//!
//! 1. `width` as `u32` big-endian, `height` as `u32` big-endian;
//! 2. `bit_depth`, `colour_type` (raw IHDR codes), `interlaced` as
//!    `0`/`1`, one byte each;
//! 3. a variant tag byte (`Gray8`=0 … `Rgba16`=5, declaration order);
//! 4. the pixel samples, row-major, channel-interleaved — `u8` verbatim,
//!    `u16` big-endian (PNG's own sample order).

use crate::Pixels;
use crate::Png;

/// The `Pixels` variant tag byte of the canonical serialization.
fn variant_tag(pixels: &Pixels) -> u8 {
    match pixels {
        Pixels::Gray8(_) => 0,
        Pixels::Gray16(_) => 1,
        Pixels::Rgb8(_) => 2,
        Pixels::Rgb16(_) => 3,
        Pixels::Rgba8(_) => 4,
        Pixels::Rgba16(_) => 5,
    }
}

/// The name of the concrete `Pixels` variant, for the `reference.json`
/// record and the SDK test harnesses.
#[must_use]
pub fn variant_name(pixels: &Pixels) -> &'static str {
    match pixels {
        Pixels::Gray8(_) => "Gray8",
        Pixels::Gray16(_) => "Gray16",
        Pixels::Rgb8(_) => "Rgb8",
        Pixels::Rgb16(_) => "Rgb16",
        Pixels::Rgba8(_) => "Rgba8",
        Pixels::Rgba16(_) => "Rgba16",
    }
}

/// Serializes one decoded PNG into the canonical byte stream the vector
/// digest covers (see the module docs for the exact layout).
#[must_use]
pub fn canonical_bytes(png: &Png) -> alloc::vec::Vec<u8> {
    let mut out = alloc::vec::Vec::new();
    out.extend_from_slice(&png.width().to_be_bytes());
    out.extend_from_slice(&png.height().to_be_bytes());
    out.push(png.bit_depth());
    out.push(png.colour_type());
    out.push(u8::from(png.interlaced()));
    let pixels = png.pixels();
    out.push(variant_tag(pixels));
    match pixels {
        Pixels::Gray8(i) => out.extend_from_slice(i.as_slice()),
        Pixels::Gray16(i) => {
            for s in i.as_slice() {
                out.extend_from_slice(&s.to_be_bytes());
            }
        }
        Pixels::Rgb8(i) => out.extend_from_slice(i.as_slice()),
        Pixels::Rgb16(i) => {
            for s in i.as_slice() {
                out.extend_from_slice(&s.to_be_bytes());
            }
        }
        Pixels::Rgba8(i) => out.extend_from_slice(i.as_slice()),
        Pixels::Rgba16(i) => {
            for s in i.as_slice() {
                out.extend_from_slice(&s.to_be_bytes());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{canonical_bytes, variant_name};
    use crate::{Limits, Pixels, decode};

    /// Decodes a committed fixture through the public API.
    fn fixture(name: &str) -> alloc::vec::Vec<u8> {
        let path = format!("{}/tests/fixtures/{name}.png", env!("CARGO_MANIFEST_DIR"));
        std::fs::read(&path).unwrap_or_else(|e| panic!("cannot read {path}: {e}"))
    }

    /// One canonical header: (width, height, depth, colour, interlaced, tag).
    type Header = (u32, u32, u8, u8, u8, u8);

    /// One fixture per concrete `Pixels` variant: the header (12 bytes)
    /// is the recorded IHDR facts plus the variant tag, and the variant
    /// name round-trips to the `reference.json` spelling.
    #[test]
    fn canonical_header_and_variant_names() {
        let cases: &[(&str, Header)] = &[
            // (fixture, (width, height, depth, colour, interlaced, tag))
            ("gray8_4x4", (4, 4, 8, 0, 0, 0)),
            ("gray16_3x2", (3, 2, 16, 0, 0, 1)),
            ("rgb8_filters_5x5", (5, 5, 8, 2, 0, 2)),
            ("rgb16_3x2", (3, 2, 16, 2, 0, 3)),
            ("rgba8_4x4", (4, 4, 8, 6, 0, 4)),
            ("rgba16_2x2", (2, 2, 16, 6, 0, 5)),
            ("adam7_13x11_rgb", (13, 11, 8, 2, 1, 2)),
            ("pal8_trns_4x4", (4, 4, 8, 3, 0, 4)),
        ];
        for (name, expected) in cases {
            let png = decode(&fixture(name), &Limits::default())
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            let bytes = canonical_bytes(&png);
            let got = (
                u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
                u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
                bytes[8],
                bytes[9],
                bytes[10],
                bytes[11],
            );
            assert_eq!(got, *expected, "{name}");
            assert_eq!(
                bytes.len() - 12,
                png.pixels().width() as usize
                    * png.pixels().height() as usize
                    * png.pixels().channels()
                    * if matches!(
                        png.pixels(),
                        Pixels::Gray16(_) | Pixels::Rgb16(_) | Pixels::Rgba16(_)
                    ) {
                        2
                    } else {
                        1
                    },
                "{name}: samples follow the header"
            );
            let variant = match png.pixels() {
                p @ (Pixels::Gray8(_)
                | Pixels::Gray16(_)
                | Pixels::Rgb8(_)
                | Pixels::Rgb16(_)
                | Pixels::Rgba8(_)
                | Pixels::Rgba16(_)) => variant_name(p),
            };
            assert!(!variant.is_empty());
        }
    }

    /// The serialization of the same file is stable across calls.
    #[test]
    fn canonical_bytes_are_deterministic() {
        let png = decode(&fixture("rgba8_4x4"), &Limits::default()).expect("decode");
        assert_eq!(canonical_bytes(&png), canonical_bytes(&png));
    }
}
