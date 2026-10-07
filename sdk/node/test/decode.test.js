// SPDX-License-Identifier: MIT
// Copyright (c) 2026 pith-hash
"use strict";

// Hex-exact conformance: the committed reference vectors through koffi.
// Every vector in the repository-root reference.json is replayed through
// the cdylib and compared byte-exact — the canonical stream's SHA-256
// against pixels_sha256, plus every recorded IHDR fact and the concrete
// Pixels variant name. The same vectors the Rust gen-reference verify
// gate and the Python/Go SDKs check.

const test = require("node:test");
const assert = require("node:assert/strict");
const crypto = require("node:crypto");
const fs = require("node:fs");
const path = require("node:path");

const { FfiError, decodeCanonical, findCdylib, parseCanonical } = require("../index.js");

const REPO_ROOT = path.resolve(__dirname, "..", "..", "..");

const REFERENCE = JSON.parse(fs.readFileSync(path.join(REPO_ROOT, "reference.json"), "utf8")).vectors;

test("cdylib is discoverable", () => {
  assert.ok(fs.statSync(findCdylib()).isFile());
});

for (const [name, vector] of Object.entries(REFERENCE)) {
  test(`reference vector ${name} is reproduced hex-exact`, () => {
    const data = fs.readFileSync(path.join(REPO_ROOT, "tests", "fixtures", `${name}.png`));

    const raw = decodeCanonical(data);
    assert.equal(crypto.createHash("sha256").update(raw).digest("hex"), vector.pixels_sha256, name);

    const canonical = parseCanonical(raw);
    assert.equal(canonical.width, vector.width, name);
    assert.equal(canonical.height, vector.height, name);
    assert.equal(canonical.bitDepth, vector.bit_depth, name);
    assert.equal(canonical.colourType, vector.colour_type, name);
    assert.equal(canonical.interlaced, vector.interlaced, name);
    assert.equal(canonical.pixels, vector.pixels, name);
  });
}

test("malformed input is refused, not crashing", () => {
  assert.throws(() => decodeCanonical(Buffer.from("not a png at all")), (err) => {
    assert.ok(err instanceof FfiError);
    assert.equal(err.status, -2);
    return true;
  });
});

test("empty input is refused", () => {
  assert.throws(() => decodeCanonical(Buffer.alloc(0)), FfiError);
});

test("full stream matches a rust-pinned value", () => {
  // gray8_4x4's digest, pinned in the committed reference.json and
  // re-derived by the Rust unit tests; this test fails loudly even if
  // reference.json were regenerated wrongly.
  const data = fs.readFileSync(path.join(REPO_ROOT, "tests", "fixtures", "gray8_4x4.png"));
  const raw = decodeCanonical(data);
  assert.equal(crypto.createHash("sha256").update(raw).digest("hex"), "dc1efcfb5d20ba31f472bca62a9c7c82824ed3e9e5ade857c6919bb2b4fbcb02");
  assert.deepEqual([...raw.subarray(0, 12)], [0, 0, 0, 4, 0, 0, 0, 4, 8, 0, 0, 0]);
});
