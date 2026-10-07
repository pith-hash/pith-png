# SPDX-License-Identifier: MIT
# Copyright (c) 2026 pith-hash
"""Hex-exact conformance: the committed reference vectors through ctypes.

Every vector in the repository-root ``reference.json`` is replayed
through the cdylib and compared byte-exact — the canonical stream's
SHA-256 against ``pixels_sha256``, plus every recorded IHDR fact and
the concrete ``Pixels`` variant name. The same vectors the Rust
``gen-reference verify`` gate and the Node/Go SDKs check.
"""

from __future__ import annotations

import hashlib
import json
from pathlib import Path

import pytest

from pith_png import FfiError, decode_canonical, find_cdylib, parse_canonical

REPO_ROOT = Path(__file__).resolve().parents[3]


def test_cdylib_is_discoverable() -> None:
    path = find_cdylib()
    assert path.is_file(), path


@pytest.mark.parametrize(
    "name", sorted(json.loads((REPO_ROOT / "reference.json").read_text(encoding="utf-8"))["vectors"])
)
def test_reference_vector_is_reproduced_hex_exact(name: str) -> None:
    vector = json.loads((REPO_ROOT / "reference.json").read_text(encoding="utf-8"))["vectors"][name]
    data = (REPO_ROOT / "tests" / "fixtures" / f"{name}.png").read_bytes()

    raw = decode_canonical(data)
    assert hashlib.sha256(raw).hexdigest() == vector["pixels_sha256"], name

    canonical = parse_canonical(raw)
    assert canonical.width == vector["width"], name
    assert canonical.height == vector["height"], name
    assert canonical.bit_depth == vector["bit_depth"], name
    assert canonical.colour_type == vector["colour_type"], name
    assert canonical.interlaced == vector["interlaced"], name
    assert canonical.pixels == vector["pixels"], name


def test_malformed_input_is_refused_not_crashing() -> None:
    with pytest.raises(FfiError) as err:
        decode_canonical(b"not a png at all")
    assert err.value.status == -2


def test_empty_input_is_refused() -> None:
    with pytest.raises(FfiError):
        decode_canonical(b"")


def test_full_stream_matches_a_rust_pinned_value() -> None:
    # gray8_4x4's digest, pinned in the committed reference.json and
    # re-derived by the Rust unit tests; this test fails loudly even if
    # reference.json were regenerated wrongly.
    data = (REPO_ROOT / "tests" / "fixtures" / "gray8_4x4.png").read_bytes()
    raw = decode_canonical(data)
    assert hashlib.sha256(raw).hexdigest() == "dc1efcfb5d20ba31f472bca62a9c7c82824ed3e9e5ade857c6919bb2b4fbcb02"
    assert raw[:12] == bytes([0, 0, 0, 4, 0, 0, 0, 4, 8, 0, 0, 0])
