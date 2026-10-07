// SPDX-License-Identifier: MIT
// Copyright (c) 2026 pith-hash

package pithpng

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
)

// repoRoot resolves the repository root relative to this package
// (sdk/go -> sdk -> repo root), the anchor for reference.json and the
// committed fixtures.
func repoRoot(t *testing.T) string {
	t.Helper()
	root, err := filepath.Abs(filepath.Join("..", ".."))
	if err != nil {
		t.Fatal(err)
	}
	if st, err := os.Stat(filepath.Join(root, "reference.json")); err != nil || st.IsDir() {
		t.Fatalf("reference.json not found at %s", root)
	}
	return root
}

// reference parses the committed reference.json.
func reference(t *testing.T) map[string]struct {
	Width      uint32 `json:"width"`
	Height     uint32 `json:"height"`
	BitDepth   uint8  `json:"bit_depth"`
	ColourType uint8  `json:"colour_type"`
	Interlaced bool   `json:"interlaced"`
	Pixels     string `json:"pixels"`
	PixelsSha  string `json:"pixels_sha256"`
} {
	t.Helper()
	raw, err := os.ReadFile(filepath.Join(repoRoot(t), "reference.json"))
	if err != nil {
		t.Fatal(err)
	}
	var parsed struct {
		Vectors map[string]struct {
			Width      uint32 `json:"width"`
			Height     uint32 `json:"height"`
			BitDepth   uint8  `json:"bit_depth"`
			ColourType uint8  `json:"colour_type"`
			Interlaced bool   `json:"interlaced"`
			Pixels     string `json:"pixels"`
			PixelsSha  string `json:"pixels_sha256"`
		} `json:"vectors"`
	}
	if err := json.Unmarshal(raw, &parsed); err != nil {
		t.Fatal(err)
	}
	return parsed.Vectors
}

// TestReferenceVectorsHexExact replays every committed reference.json
// vector through the cdylib and compares byte-exact: the canonical
// stream's SHA-256 against pixels_sha256, plus every recorded IHDR
// fact and the concrete Pixels variant name — the same vectors the
// Rust gen-reference verify gate and the Python/Node SDKs check.
func TestReferenceVectorsHexExact(t *testing.T) {
	vectors := reference(t)
	for name, want := range vectors {
		t.Run(name, func(t *testing.T) {
			data, err := os.ReadFile(filepath.Join(repoRoot(t), "tests", "fixtures", name+".png"))
			if err != nil {
				t.Fatal(err)
			}
			raw, err := DecodeCanonical(data)
			if err != nil {
				t.Fatalf("DecodeCanonical(%s): %v", name, err)
			}
			digest := sha256.Sum256(raw)
			if got := hex.EncodeToString(digest[:]); got != want.PixelsSha {
				t.Errorf("%s: digest %s, want %s", name, got, want.PixelsSha)
			}
			canonical, err := ParseCanonical(raw)
			if err != nil {
				t.Fatal(err)
			}
			if canonical.Width != want.Width || canonical.Height != want.Height {
				t.Errorf("%s: dims %dx%d, want %dx%d", name, canonical.Width, canonical.Height, want.Width, want.Height)
			}
			if canonical.BitDepth != want.BitDepth || canonical.ColourType != want.ColourType {
				t.Errorf("%s: depth/colour %d/%d, want %d/%d", name, canonical.BitDepth, canonical.ColourType, want.BitDepth, want.ColourType)
			}
			if canonical.Interlaced != want.Interlaced {
				t.Errorf("%s: interlaced %v, want %v", name, canonical.Interlaced, want.Interlaced)
			}
			if canonical.Pixels != want.Pixels {
				t.Errorf("%s: variant %s, want %s", name, canonical.Pixels, want.Pixels)
			}
		})
	}
}

// TestGray8PinnedDigest pins one digest the Rust unit tests re-derive,
// so the binding fails loudly even if reference.json were regenerated
// wrongly.
func TestGray8PinnedDigest(t *testing.T) {
	data, err := os.ReadFile(filepath.Join(repoRoot(t), "tests", "fixtures", "gray8_4x4.png"))
	if err != nil {
		t.Fatal(err)
	}
	raw, err := DecodeCanonical(data)
	if err != nil {
		t.Fatal(err)
	}
	digest := sha256.Sum256(raw)
	const want = "dc1efcfb5d20ba31f472bca62a9c7c82824ed3e9e5ade857c6919bb2b4fbcb02"
	if got := hex.EncodeToString(digest[:]); got != want {
		t.Errorf("gray8_4x4: digest %s, want %s", got, want)
	}
	wantHeader := []byte{0, 0, 0, 4, 0, 0, 0, 4, 8, 0, 0, 0}
	for i, b := range wantHeader {
		if raw[i] != b {
			t.Fatalf("gray8_4x4: header byte %d = %d, want %d", i, raw[i], b)
		}
	}
}

// TestMalformedInputIsRefused checks the decoder's refusal path: a
// status code, never a crash.
func TestMalformedInputIsRefused(t *testing.T) {
	_, err := DecodeCanonical([]byte("not a png at all"))
	var ffi *FfiError
	if e, ok := err.(*FfiError); ok {
		ffi = e
	} else {
		t.Fatalf("want FfiError, got %v", err)
	}
	if ffi.Status != StatusRejected {
		t.Errorf("want StatusRejected, got %d", ffi.Status)
	}
}
