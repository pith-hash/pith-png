# SPDX-License-Identifier: MIT
# Copyright (c) 2026 pith-hash
"""pith-png SDK: PNG decoding through ctypes.

The single Rust core (the ``pith-png`` cdylib built by
``cargo build --release``) is loaded at runtime; this package carries
no third-party dependency — ``ctypes`` is the standard library.

Discovery order (the suite's cdylib convention):

1. ``PITH_CDYLIB`` — an explicit cdylib *file* path;
2. ``PITH_CDYLIB_DIR`` — a *directory* scanned for the cdylib names
   (the CD pipeline points this at ``target/release``);
3. the package directory itself (the built wheel ships the cdylib as
   package data);
4. ``<repo root>/target/release`` — the repository working-tree layout,
   so a source checkout runs against a local cargo build with no
   configuration.

The FFI surface is one decode operation plus one free:
``pith_png_decode_canonical`` decodes a whole PNG stream into the
canonical byte stream the ``reference.json`` vectors are defined over
(width/height big-endian, IHDR facts, variant tag, samples), and
``pith_png_free`` releases the handed-out buffer.
"""

from __future__ import annotations

import ctypes
import os
from dataclasses import dataclass
from pathlib import Path

__all__ = [
    "Canonical",
    "FfiError",
    "LibraryNotFoundError",
    "find_cdylib",
    "decode_canonical",
    "parse_canonical",
    "PIXELS_VARIANTS",
    "STATUS_OK",
    "STATUS_INVALID",
    "STATUS_REJECTED",
]

#: Status: success.
STATUS_OK = 0
#: Status: a caller argument is invalid (a null pointer).
STATUS_INVALID = -1
#: Status: the core decoder refused the input (malformed PNG).
STATUS_REJECTED = -2

#: The `Pixels` variant names by canonical tag byte (declaration order).
PIXELS_VARIANTS = ("Gray8", "Gray16", "Rgb8", "Rgb16", "Rgba8", "Rgba16")

#: Every cdylib file name cargo may drop into the build directory, per
#: platform (windows / linux / macOS).
CDYLIB_NAMES = ("pith_png.dll", "libpith_png.so", "libpith_png.dylib")


@dataclass(frozen=True)
class Canonical:
    """The decoded PNG, re-expressed from the canonical byte stream.

    ``samples`` is the row-major, channel-interleaved pixel buffer —
    ``u8`` verbatim, ``u16`` big-endian (PNG's own sample order) —
    exactly the bytes the ``pixels_sha256`` digest covers.
    """

    #: Image width in pixels.
    width: int
    #: Image height in pixels.
    height: int
    #: Bit depth the file was coded at (1..16), not necessarily the
    #: sample width of ``samples``.
    bit_depth: int
    #: Colour type code the file declared (0, 2, 3, 4 or 6).
    colour_type: int
    #: Whether the file was Adam7-interlaced.
    interlaced: bool
    #: The concrete ``Pixels`` variant name (``Gray8`` … ``Rgba16``).
    pixels: str
    #: The canonical byte stream the digest is computed over.
    raw: bytes

    @property
    def samples(self) -> bytes:
        """The pixel samples (everything after the 12-byte header)."""
        return self.raw[12:]


class LibraryNotFoundError(OSError):
    """No cdylib was found through the discovery chain."""


class FfiError(Exception):
    """A non-zero status code came back from the cdylib."""

    def __init__(self, op: str, status: int) -> None:
        kind = {
            STATUS_INVALID: "invalid argument",
            STATUS_REJECTED: "input rejected",
        }.get(status, "unknown failure")
        super().__init__(f"{op} failed: {kind} (status {status})")
        #: The raw status code the FFI returned.
        self.status = status


def find_cdylib() -> Path:
    """Locates the cdylib through the suite's discovery chain."""
    explicit = os.environ.get("PITH_CDYLIB")
    if explicit:
        p = Path(explicit)
        if p.is_file():
            return p
    env_dir = os.environ.get("PITH_CDYLIB_DIR")
    candidates: list[Path] = []
    if env_dir:
        env_dir_path = Path(env_dir)
        candidates.append(env_dir_path)
        if not env_dir_path.is_absolute():
            # CD and local runs invoke tools from the repository root or
            # from sdk/<lang>; resolve the env value against both.
            candidates.append(Path.cwd() / env_dir_path)
            candidates.append(Path(__file__).resolve().parents[3] / env_dir_path)
    candidates.append(Path(__file__).resolve().parent)  # packaged wheel
    candidates.append(Path(__file__).resolve().parents[3] / "target" / "release")
    for directory in candidates:
        for name in CDYLIB_NAMES:
            p = directory / name
            if p.is_file():
                return p
    raise LibraryNotFoundError(
        "no pith-png cdylib found (searched PITH_CDYLIB, PITH_CDYLIB_DIR, "
        "the package directory and <repo>/target/release); "
        "run `cargo build --release` first"
    )


_lib: ctypes.CDLL | None = None


def _load() -> ctypes.CDLL:
    global _lib
    if _lib is None:
        lib = ctypes.CDLL(str(find_cdylib()))
        lib.pith_png_decode_canonical.argtypes = [
            ctypes.c_void_p,  # data
            ctypes.c_size_t,  # len
            ctypes.POINTER(ctypes.c_void_p),  # out buffer
            ctypes.POINTER(ctypes.c_size_t),  # out length
        ]
        lib.pith_png_decode_canonical.restype = ctypes.c_int32
        lib.pith_png_free.argtypes = [ctypes.c_void_p, ctypes.c_size_t]
        lib.pith_png_free.restype = None
        _lib = lib
    return _lib


def decode_canonical(data: bytes) -> bytes:
    """Decodes a complete PNG stream into the canonical byte stream the
    ``reference.json`` vectors are defined over.

    Raises :class:`FfiError` with ``status == STATUS_REJECTED`` for any
    malformed input — bad signature, CRC, chunk order or a truncated
    stream; the decoder never panics through this boundary.
    """
    out = ctypes.c_void_p()
    out_len = ctypes.c_size_t()
    status = _load().pith_png_decode_canonical(data, len(data), ctypes.byref(out), ctypes.byref(out_len))
    if status != STATUS_OK:
        raise FfiError("pith_png_decode_canonical", status)
    try:
        return ctypes.string_at(out, out_len.value)
    finally:
        _load().pith_png_free(out, out_len.value)


def parse_canonical(raw: bytes) -> Canonical:
    """Re-expresses the canonical byte stream as a :class:`Canonical`."""
    if len(raw) < 12:
        raise ValueError("canonical stream is shorter than the 12-byte header")
    width = int.from_bytes(raw[0:4], "big")
    height = int.from_bytes(raw[4:8], "big")
    tag = raw[11]
    if tag >= len(PIXELS_VARIANTS):
        raise ValueError(f"unknown variant tag {tag}")
    return Canonical(
        width=width,
        height=height,
        bit_depth=raw[8],
        colour_type=raw[9],
        interlaced=raw[10] == 1,
        pixels=PIXELS_VARIANTS[tag],
        raw=raw,
    )
