// SPDX-License-Identifier: MIT
// Copyright (c) 2026 pith-hash

// Package pithpng provides Go bindings for the pith-png Rust cdylib:
// PNG decoding into the canonical vector stream.
//
// The single Rust core (built by `cargo build --release`) is loaded at
// runtime; the package carries zero module dependencies. On unix the
// cdylib is opened with dlopen through cgo, on Windows with
// LoadLibrary through the standard syscall package — both resolve the
// library through the same discovery chain, so `go build ./... &&
// go test ./...` works unchanged on every OS the CD matrix builds.
//
// Discovery order (the suite's cdylib convention):
//
//  1. PITH_CDYLIB — an explicit cdylib file path;
//  2. PITH_CDYLIB_DIR — a directory scanned for the cdylib names (the
//     CD pipeline points this at target/release);
//  3. <repo root>/target/release — the repository working-tree layout,
//     anchored at this package's source directory, so a source
//     checkout runs against a local cargo build unconfigured.
//
// The FFI surface is one decode operation plus one free:
// pith_png_decode_canonical decodes a whole PNG stream into the
// canonical byte stream the reference.json vectors are defined over
// (width/height big-endian, IHDR facts, variant tag, samples), and
// pith_png_free releases the handed-out buffer.
package pithpng

import (
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"sync"
	"unsafe"
)

// Status codes returned by the cdylib's C ABI.
const (
	// StatusOK: success.
	StatusOK int32 = 0
	// StatusInvalid: a caller argument is invalid (a null pointer).
	StatusInvalid int32 = -1
	// StatusRejected: the core decoder refused the input (malformed
	// PNG: bad signature, CRC, chunk order, or truncated stream).
	StatusRejected int32 = -2
)

// PixelsVariants names the decoded `Pixels` variants by canonical tag
// byte (declaration order).
var PixelsVariants = [...]string{"Gray8", "Gray16", "Rgb8", "Rgb16", "Rgba8", "Rgba16"}

// cdylibNames are the file names cargo may drop into the build
// directory, per platform (windows / linux / macOS).
var cdylibNames = []string{"pith_png.dll", "libpith_png.so", "libpith_png.dylib"}

// FfiError reports a non-zero status code from the cdylib.
type FfiError struct {
	// Op is the FFI operation name.
	Op string
	// Status is the raw status code the FFI returned.
	Status int32
}

func (e *FfiError) Error() string {
	kind := "unknown failure"
	switch e.Status {
	case StatusInvalid:
		kind = "invalid argument"
	case StatusRejected:
		kind = "input rejected"
	}
	return fmt.Sprintf("%s failed: %s (status %d)", e.Op, kind, e.Status)
}

// FindCdylib locates the cdylib through the suite's discovery chain.
func FindCdylib() (string, error) {
	if p := os.Getenv("PITH_CDYLIB"); p != "" {
		if st, err := os.Stat(p); err == nil && st.Mode().IsRegular() {
			return filepath.Abs(p)
		}
	}
	_, thisFile, _, ok := runtime.Caller(0)
	if !ok {
		return "", fmt.Errorf("pithpng: cannot locate the package source directory")
	}
	pkgDir := filepath.Dir(thisFile)
	repoRoot := filepath.Dir(filepath.Dir(pkgDir)) // sdk/go -> sdk -> repo root

	var dirs []string
	if env := os.Getenv("PITH_CDYLIB_DIR"); env != "" {
		dirs = append(dirs, env)
		if !filepath.IsAbs(env) {
			dirs = append(dirs, filepath.Join(repoRoot, env))
		}
	}
	dirs = append(dirs, filepath.Join(repoRoot, "target", "release"))
	for _, dir := range dirs {
		for _, name := range cdylibNames {
			p := filepath.Join(dir, name)
			if st, err := os.Stat(p); err == nil && st.Mode().IsRegular() {
				return p, nil
			}
		}
	}
	return "", fmt.Errorf(
		"pithpng: no cdylib found (searched PITH_CDYLIB, PITH_CDYLIB_DIR and <repo>/target/release); run `cargo build --release` first",
	)
}

// locate resolves the cdylib path once per process.
var locate = sync.OnceValues(FindCdylib)

// Canonical is the decoded PNG, re-expressed from the canonical byte
// stream: the IHDR facts, the concrete Pixels variant and the samples.
type Canonical struct {
	// Width is the image width in pixels.
	Width uint32
	// Height is the image height in pixels.
	Height uint32
	// BitDepth is the bit depth the file was coded at (1..16), not
	// necessarily the sample width of Samples.
	BitDepth uint8
	// ColourType is the colour type code the file declared (0, 2, 3, 4 or 6).
	ColourType uint8
	// Interlaced reports Adam7 interlacing.
	Interlaced bool
	// Pixels is the concrete variant name (Gray8 … Rgba16).
	Pixels string
	// Samples is the row-major, channel-interleaved pixel buffer —
	// u8 verbatim, u16 big-endian (PNG's own sample order).
	Samples []byte
}

// DecodeCanonical decodes a complete PNG stream into the canonical
// byte stream the reference.json vectors are defined over. The
// returned slice is a Go copy; the handed-out cdylib buffer is
// released before returning.
func DecodeCanonical(data []byte) ([]byte, error) {
	libPath, err := locate()
	if err != nil {
		return nil, err
	}
	var out *byte
	var outLen uintptr
	var dataPtr *byte
	if len(data) > 0 {
		dataPtr = &data[0]
	}
	status, err := ffiDecodeCanonical(libPath, dataPtr, len(data), &out, &outLen)
	if err != nil {
		return nil, err
	}
	if status != StatusOK {
		return nil, &FfiError{Op: "pith_png_decode_canonical", Status: status}
	}
	buf := make([]byte, outLen)
	copy(buf, unsafe.Slice(out, outLen))
	ffiFree(libPath, out, outLen)
	return buf, nil
}

// ParseCanonical re-expresses the canonical byte stream as a Canonical.
func ParseCanonical(raw []byte) (Canonical, error) {
	if len(raw) < 12 {
		return Canonical{}, fmt.Errorf("pithpng: canonical stream is shorter than the 12-byte header")
	}
	tag := raw[11]
	if int(tag) >= len(PixelsVariants) {
		return Canonical{}, fmt.Errorf("pithpng: unknown variant tag %d", tag)
	}
	return Canonical{
		Width:      uint32(raw[0])<<24 | uint32(raw[1])<<16 | uint32(raw[2])<<8 | uint32(raw[3]),
		Height:     uint32(raw[4])<<24 | uint32(raw[5])<<16 | uint32(raw[6])<<8 | uint32(raw[7]),
		BitDepth:   raw[8],
		ColourType: raw[9],
		Interlaced: raw[10] == 1,
		Pixels:     PixelsVariants[tag],
		Samples:    raw[12:],
	}, nil
}
