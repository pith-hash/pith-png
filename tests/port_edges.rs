//! Edge-condition tests added at port time, targeting the decoder's
//! structural refusals and colour-type/tRNS combinations the conformance
//! fixtures do not carry (16-bit `tRNS` expansion, 2-bit depth scaling,
//! multi-block encoder output). Everything else in this suite is ported
//! verbatim from the upstream monorepo suite; these tests only ever
//! sharpen the checks, never weaken them.

use pith_digest::{Error, crc32};
use pith_png::{Gray, Image, Limits, Pixels, Rgb, Rgba, decode, decode_ihdr, encode};

const GRAY8: &[u8] = include_bytes!("fixtures/gray8_4x4.png");
const GRAY16: &[u8] = include_bytes!("fixtures/gray16_3x2.png");
const RGB8: &[u8] = include_bytes!("fixtures/rgb8_filters_5x5.png");
const RGB16: &[u8] = include_bytes!("fixtures/rgb16_3x2.png");
const RGBA8: &[u8] = include_bytes!("fixtures/rgba8_4x4.png");
const RGBA16: &[u8] = include_bytes!("fixtures/rgba16_2x2.png");
const PAL8_TRNS: &[u8] = include_bytes!("fixtures/pal8_trns_4x4.png");

// ---------------------------------------------------------------------
// Test-local chunk helpers (mirrors tests/png.rs; integration tests
// cannot see each other's private items)
// ---------------------------------------------------------------------

fn chunk(ty: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(ty);
    out.extend_from_slice(data);
    let mut crc_input = Vec::with_capacity(4 + data.len());
    crc_input.extend_from_slice(ty);
    crc_input.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
    out
}

fn ihdr_chunk(w: u32, h: u32, depth: u8, colour: u8, interlace: u8) -> Vec<u8> {
    let mut d = Vec::new();
    d.extend_from_slice(&w.to_be_bytes());
    d.extend_from_slice(&h.to_be_bytes());
    d.extend_from_slice(&[depth, colour, 0, 0, interlace]);
    chunk(b"IHDR", &d)
}

fn stream(parts: &[Vec<u8>]) -> Vec<u8> {
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    for p in parts {
        out.extend_from_slice(p);
    }
    out
}

/// A stored-block zlib stream for hand-built test rows, mirroring the
/// encoder's wrapper but local so tests do not trust the code under test
/// to build its own adversarial inputs.
fn test_zlib(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let mut at = 0usize;
    loop {
        let take = (data.len() - at).min(65_535);
        let last = at + take == data.len();
        out.push(if last { 1 } else { 0 });
        out.extend_from_slice(&(take as u16).to_le_bytes());
        out.extend_from_slice(&(!(take as u16)).to_le_bytes());
        out.extend_from_slice(&data[at..at + take]);
        at += take;
        if last {
            break;
        }
    }
    out.extend_from_slice(&test_adler32(data).to_be_bytes());
    out
}

/// Adler-32 recomputed in the test, not imported from the suite: a
/// checker that shares its arithmetic with the code under test checks
/// nothing.
fn test_adler32(data: &[u8]) -> u32 {
    let mut a = 1u32;
    let mut b = 0u32;
    for &byte in data {
        a = (a + byte as u32) % 65_521;
        b = (b + a) % 65_521;
    }
    (b << 16) | a
}

/// Flips the first data byte of the first chunk of type `ty`, without
/// repairing the CRC, so decode must fail at the integrity check.
fn corrupt_chunk_data(file: &[u8], ty: &[u8; 4]) -> Vec<u8> {
    let mut m = file.to_vec();
    let mut at = 8usize;
    while at + 12 <= m.len() {
        let len = u32::from_be_bytes([m[at], m[at + 1], m[at + 2], m[at + 3]]) as usize;
        if &m[at + 4..at + 8] == ty {
            m[at + 8] ^= 0xFF;
            return m;
        }
        at += 12 + len;
    }
    panic!("{ty:?} chunk not found");
}

// ---------------------------------------------------------------------
// decode_ihdr edge cases
// ---------------------------------------------------------------------

#[test]
fn decode_ihdr_rejects_non_ihdr_first_chunk() {
    let ancillary = chunk(b"tEXt", b"k=v");
    assert_eq!(
        decode_ihdr(&stream(&[ancillary])),
        Err(Error::BadValue("PNG first chunk is not IHDR"))
    );
}

#[test]
fn decode_ihdr_rejects_wrong_ihdr_length() {
    // 12-byte body: the length check fires before field validation.
    let mut d = Vec::new();
    d.extend_from_slice(&1u32.to_be_bytes());
    d.extend_from_slice(&1u32.to_be_bytes());
    d.extend_from_slice(&[8, 0, 0, 0]);
    let short = chunk(b"IHDR", &d);
    assert_eq!(d.len(), 12);
    assert_eq!(
        decode_ihdr(&stream(std::slice::from_ref(&short))),
        Err(Error::BadValue("PNG IHDR length"))
    );
    assert_eq!(
        decode(&stream(&[short, chunk(b"IEND", &[])]), &Limits::default()),
        Err(Error::BadValue("PNG IHDR length"))
    );
}

// ---------------------------------------------------------------------
// CRC-mismatch naming for the chunks the fixtures can carry
// ---------------------------------------------------------------------

#[test]
fn crc_mismatch_names_plte_and_trns() {
    assert_eq!(
        decode(&corrupt_chunk_data(PAL8_TRNS, b"PLTE"), &Limits::default()),
        Err(Error::BadValue("PNG PLTE CRC32 mismatch"))
    );
    assert_eq!(
        decode(&corrupt_chunk_data(PAL8_TRNS, b"tRNS"), &Limits::default()),
        Err(Error::BadValue("PNG tRNS CRC32 mismatch"))
    );
}

// ---------------------------------------------------------------------
// PLTE structural violations
// ---------------------------------------------------------------------

/// A minimal valid truecolour stream: one 1x1 row, filter 0, pixel
/// (1, 2, 3).
fn rgb8_stream() -> Vec<u8> {
    let img = Pixels::Rgb8(Image::from_vec(1, 1, vec![1, 2, 3]).unwrap());
    encode(&img, 0).unwrap()
}

#[test]
fn duplicate_plte_rejected() {
    let real = rgb8_stream();
    let idat = {
        let mut at = 8usize;
        let mut found = Vec::new();
        while at + 12 <= real.len() {
            let len =
                u32::from_be_bytes([real[at], real[at + 1], real[at + 2], real[at + 3]]) as usize;
            if &real[at + 4..at + 8] == b"IDAT" {
                found = real[at..at + 12 + len].to_vec();
                break;
            }
            at += 12 + len;
        }
        found
    };
    let plte = chunk(b"PLTE", &[0, 0, 0, 0, 0, 0]);
    let iend = chunk(b"IEND", &[]);
    assert_eq!(
        decode(
            &stream(&[ihdr_chunk(1, 1, 8, 2, 0), plte.clone(), plte, idat, iend]),
            &Limits::default()
        ),
        Err(Error::BadValue("PNG duplicate PLTE"))
    );
}

#[test]
fn plte_in_greyscale_rejected() {
    let plte = chunk(b"PLTE", &[0, 0, 0, 0, 0, 0]);
    let idat = chunk(b"IDAT", &test_zlib(&[0, 42]));
    assert_eq!(
        decode(
            &stream(&[ihdr_chunk(1, 1, 8, 0, 0), plte, idat, chunk(b"IEND", &[])]),
            &Limits::default()
        ),
        Err(Error::BadValue("PNG PLTE in greyscale image"))
    );
}

#[test]
fn plte_length_rules() {
    let iend = chunk(b"IEND", &[]);
    for bad in [
        &[][..],           // empty
        &[1, 2, 3, 4][..], // not a multiple of 3
        &[0; 771][..],     // 257 entries, one past the 256 ceiling
    ] {
        let plte = chunk(b"PLTE", bad);
        let idat = chunk(b"IDAT", &test_zlib(&[0, 1, 2, 3]));
        assert_eq!(
            decode(
                &stream(&[ihdr_chunk(1, 1, 8, 2, 0), plte, idat, iend.clone()]),
                &Limits::default()
            ),
            Err(Error::BadValue("PNG PLTE length")),
            "PLTE body of {} bytes must be refused",
            bad.len()
        );
    }
}

// ---------------------------------------------------------------------
// tRNS structural violations
// ---------------------------------------------------------------------

#[test]
fn trns_after_idat_rejected() {
    let idat = chunk(b"IDAT", &test_zlib(&[0, 1, 2, 3]));
    let trns = chunk(b"tRNS", &[0, 0, 0, 0, 0, 0]);
    assert_eq!(
        decode(
            &stream(&[ihdr_chunk(1, 1, 8, 2, 0), idat, trns, chunk(b"IEND", &[])]),
            &Limits::default()
        ),
        Err(Error::BadValue("PNG tRNS after IDAT"))
    );
}

#[test]
fn duplicate_trns_rejected() {
    let trns = chunk(b"tRNS", &[0, 0]);
    let idat = chunk(b"IDAT", &test_zlib(&[0, 42]));
    assert_eq!(
        decode(
            &stream(&[
                ihdr_chunk(1, 1, 8, 0, 0),
                trns.clone(),
                trns,
                idat,
                chunk(b"IEND", &[])
            ]),
            &Limits::default()
        ),
        Err(Error::BadValue("PNG duplicate tRNS"))
    );
}

#[test]
fn trns_length_rules() {
    let iend = chunk(b"IEND", &[]);
    // Greyscale: exactly 2 bytes.
    let idat = chunk(b"IDAT", &test_zlib(&[0, 42]));
    assert_eq!(
        decode(
            &stream(&[
                ihdr_chunk(1, 1, 8, 0, 0),
                chunk(b"tRNS", &[0, 0, 0]),
                idat,
                iend.clone()
            ]),
            &Limits::default()
        ),
        Err(Error::BadValue("PNG tRNS length"))
    );
    // Truecolour: exactly 6 bytes.
    let idat = chunk(b"IDAT", &test_zlib(&[0, 1, 2, 3]));
    assert_eq!(
        decode(
            &stream(&[
                ihdr_chunk(1, 1, 8, 2, 0),
                chunk(b"tRNS", &[0, 0, 0, 0, 0]),
                idat,
                iend.clone()
            ]),
            &Limits::default()
        ),
        Err(Error::BadValue("PNG tRNS length"))
    );
    // Indexed: at least one byte, at most one per PLTE entry. One
    // palette entry (3 PLTE bytes), so a 2-byte tRNS is over.
    let idat = chunk(b"IDAT", &test_zlib(&[0, 0]));
    for body in [&[][..], &[0, 0][..]] {
        let s = stream(&[
            ihdr_chunk(1, 1, 8, 3, 0),
            chunk(b"PLTE", &[0, 0, 0]),
            chunk(b"tRNS", body),
            idat.clone(),
            iend.clone(),
        ]);
        assert_eq!(
            decode(&s, &Limits::default()),
            Err(Error::BadValue("PNG tRNS length")),
            "tRNS body of {} bytes must be refused",
            body.len()
        );
    }
}

#[test]
fn indexed_trns_before_plte_rejected() {
    let idat = chunk(b"IDAT", &test_zlib(&[0, 0]));
    assert_eq!(
        decode(
            &stream(&[
                ihdr_chunk(1, 1, 8, 3, 0),
                chunk(b"tRNS", &[0]),
                idat,
                chunk(b"IEND", &[])
            ]),
            &Limits::default()
        ),
        Err(Error::BadValue("PNG tRNS before PLTE"))
    );
}

// ---------------------------------------------------------------------
// IEND and input ceilings
// ---------------------------------------------------------------------

#[test]
fn iend_with_body_rejected() {
    let idat = chunk(b"IDAT", &test_zlib(&[0, 42]));
    assert_eq!(
        decode(
            &stream(&[ihdr_chunk(1, 1, 8, 0, 0), idat, chunk(b"IEND", &[0])]),
            &Limits::default()
        ),
        Err(Error::BadValue("PNG IEND length"))
    );
}

#[test]
fn input_over_limit_is_too_large() {
    let limits = Limits {
        max_input: 16,
        ..Limits::default()
    };
    match decode(GRAY8, &limits) {
        Err(Error::TooLarge { what, limit }) => {
            assert_eq!(what, "PNG input");
            assert_eq!(limit, 16);
        }
        other => panic!("expected TooLarge, got {other:?}"),
    }
}

// ---------------------------------------------------------------------
// Pixels accessors across every variant
// ---------------------------------------------------------------------

#[test]
fn pixels_accessors_report_every_variant() {
    let cases: &[(&[u8], &str, usize)] = &[
        (GRAY8, "Gray8", 1),
        (GRAY16, "Gray16", 1),
        (RGB8, "Rgb8", 3),
        (RGB16, "Rgb16", 3),
        (RGBA8, "Rgba8", 4),
        (RGBA16, "Rgba16", 4),
    ];
    for (file, variant, channels) in cases {
        let png = decode(file, &Limits::default()).unwrap();
        let pixels = png.pixels();
        assert_eq!(
            *variant,
            match pixels {
                Pixels::Gray8(_) => "Gray8",
                Pixels::Gray16(_) => "Gray16",
                Pixels::Rgb8(_) => "Rgb8",
                Pixels::Rgb16(_) => "Rgb16",
                Pixels::Rgba8(_) => "Rgba8",
                Pixels::Rgba16(_) => "Rgba16",
            }
        );
        assert_eq!(pixels.width(), png.width());
        assert_eq!(pixels.height(), png.height());
        assert_eq!(pixels.channels(), *channels);
    }
}

// ---------------------------------------------------------------------
// Colour-type / depth / tRNS combinations the fixtures do not carry
// ---------------------------------------------------------------------

#[test]
fn two_bit_greyscale_scales_by_eightyfive() {
    // Two 1-pixel rows, 2 bits each: row 0 codes sample 2 (0b10), row 1
    // sample 1 (0b01), MSB-first per specification section 7.2.
    let idat = chunk(b"IDAT", &test_zlib(&[0, 0b1000_0000, 0, 0b0100_0000]));
    let png = decode(
        &stream(&[ihdr_chunk(1, 2, 2, 0, 0), idat, chunk(b"IEND", &[])]),
        &Limits::default(),
    )
    .unwrap();
    // Bit replication: 2 -> 0b10101010 = 170, 1 -> 0b01010101 = 85.
    match png.pixels() {
        Pixels::Gray8(img) => assert_eq!(img.as_slice(), &[170, 85]),
        other => panic!("expected Gray8, got {other:?}"),
    }
}

#[test]
fn sixteen_bit_greyscale_trns_expands_to_rgba16() {
    // One row, two big-endian grey16 samples: 0x0005 (matches tRNS) and
    // 0x0007 (opaque).
    let idat = chunk(b"IDAT", &test_zlib(&[0, 0, 5, 0, 7]));
    let png = decode(
        &stream(&[
            ihdr_chunk(2, 1, 16, 0, 0),
            chunk(b"tRNS", &[0, 5]),
            idat,
            chunk(b"IEND", &[]),
        ]),
        &Limits::default(),
    )
    .unwrap();
    match png.pixels() {
        Pixels::Rgba16(img) => {
            assert_eq!(img.as_slice(), &[5, 5, 5, 0, 7, 7, 7, 0xffff])
        }
        other => panic!("expected Rgba16, got {other:?}"),
    }
}

#[test]
fn sixteen_bit_truecolour_trns_expands_to_rgba16() {
    // Two rgb16 pixels: (1,2,3) matches tRNS, (9,2,3) does not.
    let idat = chunk(
        b"IDAT",
        &test_zlib(&[0, 0, 1, 0, 2, 0, 3, 0, 9, 0, 2, 0, 3]),
    );
    let png = decode(
        &stream(&[
            ihdr_chunk(2, 1, 16, 2, 0),
            chunk(b"tRNS", &[0, 1, 0, 2, 0, 3]),
            idat,
            chunk(b"IEND", &[]),
        ]),
        &Limits::default(),
    )
    .unwrap();
    match png.pixels() {
        Pixels::Rgba16(img) => {
            assert_eq!(img.as_slice(), &[1, 2, 3, 0, 9, 2, 3, 0xffff])
        }
        other => panic!("expected Rgba16, got {other:?}"),
    }
}

#[test]
fn eight_bit_greyscale_trns_keeps_opaque_pixels_opaque() {
    // Sample 0 matches tRNS grey=0; sample 9 does not.
    let idat = chunk(b"IDAT", &test_zlib(&[0, 0, 9]));
    let png = decode(
        &stream(&[
            ihdr_chunk(2, 1, 8, 0, 0),
            chunk(b"tRNS", &[0, 0]),
            idat,
            chunk(b"IEND", &[]),
        ]),
        &Limits::default(),
    )
    .unwrap();
    match png.pixels() {
        Pixels::Rgba8(img) => {
            assert_eq!(img.as_slice(), &[0, 0, 0, 0, 9, 9, 9, 255])
        }
        other => panic!("expected Rgba8, got {other:?}"),
    }
}

// ---------------------------------------------------------------------
// Encoder: stored-block wrap spans multiple DEFLATE blocks
// ---------------------------------------------------------------------

#[test]
fn encoder_multi_block_zlib_round_trips() {
    // 130x130 RGBA8 = 67 730 scanline bytes > one 65 535-byte block.
    let w = 130usize;
    let h = 130usize;
    let v: Vec<u8> = (0..w * h * 4).map(|i| (i % 251) as u8).collect();
    let img = Pixels::Rgba8(Image::<Rgba, u8>::from_vec(w as u32, h as u32, v.clone()).unwrap());
    let bytes = encode(&img, 0).unwrap();
    let back = decode(&bytes, &Limits::default()).unwrap();
    assert_eq!(back.pixels(), &img);
    // And the layout aliases used by downstream crates stay importable.
    let _: Image<Gray, u8> = Image::from_vec(1, 1, vec![0]).unwrap();
    let _: Image<Rgb, u16> = Image::from_vec(1, 1, vec![0; 3]).unwrap();
}
