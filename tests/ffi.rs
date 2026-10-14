//! The C ABI surface exercised through the library build the cdylib ships.
//!
//! These tests live outside `src/ffi.rs` on purpose. The `extern "C"`
//! functions carry `#[no_mangle]` names, so a unit-test build of the crate
//! and the shipped build define the same symbol under two different coverage
//! hashes. When both exist, `llvm-cov` keeps whichever object it reads first
//! and the zero-count copy can win, which made the coverage gate depend on
//! the hash-derived order of the test binaries. With `mod ffi` compiled out
//! of the unit-test build, every copy of these functions shares one hash and
//! the counts from this binary are merged into all of them.

use pith_png::ffi::{
    PITH_E_INVALID, PITH_E_REJECTED, PITH_OK, pith_png_decode_canonical, pith_png_free,
};
use pith_png::reference::canonical_bytes;
use pith_png::{Limits, decode};

const RGBA8: &[u8] = include_bytes!("fixtures/rgba8_4x4.png");

/// A committed conformance fixture, decoded end-to-end through the raw FFI:
/// status OK, the length matches the serialization, and the buffer
/// round-trips through `pith_png_free`.
#[test]
fn ffi_decode_reproduces_the_canonical_stream() {
    let expected = canonical_bytes(&decode(RGBA8, &Limits::default()).expect("decode"));

    let mut out: *mut u8 = core::ptr::null_mut();
    let mut out_len: usize = 0;
    let status =
        unsafe { pith_png_decode_canonical(RGBA8.as_ptr(), RGBA8.len(), &mut out, &mut out_len) };
    assert_eq!(status, PITH_OK);
    assert_eq!(out_len, expected.len());
    let handed_back = unsafe { core::slice::from_raw_parts(out, out_len) };
    assert_eq!(handed_back, expected.as_slice());
    // The first 12 bytes are the documented header: 4x4, depth 8, colour
    // type 6, not interlaced, Rgba8 tag.
    assert_eq!(&handed_back[..12], &[0, 0, 0, 4, 0, 0, 0, 4, 8, 6, 0, 4]);
    unsafe { pith_png_free(out, out_len) };
}

/// Null pointers are `PITH_E_INVALID`; garbage input is `PITH_E_REJECTED`;
/// a null buffer is a legal free.
#[test]
fn ffi_refusals() {
    let mut out: *mut u8 = core::ptr::null_mut();
    let mut out_len: usize = 0;
    let null_data =
        unsafe { pith_png_decode_canonical(core::ptr::null(), 0, &mut out, &mut out_len) };
    assert_eq!(null_data, PITH_E_INVALID);

    let png = [0u8; 16];
    let null_out = unsafe {
        pith_png_decode_canonical(png.as_ptr(), png.len(), core::ptr::null_mut(), &mut out_len)
    };
    assert_eq!(null_out, PITH_E_INVALID);

    let null_len = unsafe {
        pith_png_decode_canonical(png.as_ptr(), png.len(), &mut out, core::ptr::null_mut())
    };
    assert_eq!(null_len, PITH_E_INVALID);

    let garbage =
        unsafe { pith_png_decode_canonical(png.as_ptr(), png.len(), &mut out, &mut out_len) };
    assert_eq!(garbage, PITH_E_REJECTED);

    let truncated = &RGBA8[..RGBA8.len() / 2];
    let status = unsafe {
        pith_png_decode_canonical(truncated.as_ptr(), truncated.len(), &mut out, &mut out_len)
    };
    assert_eq!(status, PITH_E_REJECTED);

    unsafe { pith_png_free(core::ptr::null_mut(), 0) };
}
