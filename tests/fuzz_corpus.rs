//! Replays the committed fuzz corpus (`fuzz/corpus/png/`) through the
//! decoder: every seed must decode cleanly, decode deterministically
//! (twice, byte-equal), and match the input digest pinned in
//! `fuzz/corpus/PROVENANCE.md` — a corrupted or drifted seed is a test
//! failure before any decode runs. The same seeds back the workspace
//! fuzz harness's `png` target; keeping them here makes every
//! `cargo test` a corpus regression run.

use pith_digest::sha256;
use pith_png::{Limits, decode};

const SEEDS: &[(&str, &[u8], &str)] = &[
    (
        "phash_gray16_36x28.png",
        include_bytes!("../fuzz/corpus/png/phash_gray16_36x28.png"),
        "08f72c40e384f114",
    ),
    (
        "phash_gray8_40x32.png",
        include_bytes!("../fuzz/corpus/png/phash_gray8_40x32.png"),
        "c43197243100470d",
    ),
    (
        "phash_pal8_trns_52x44.png",
        include_bytes!("../fuzz/corpus/png/phash_pal8_trns_52x44.png"),
        "b10baabba07f1aa8",
    ),
    (
        "phash_rgb16_40x24.png",
        include_bytes!("../fuzz/corpus/png/phash_rgb16_40x24.png"),
        "0eb2174fc167efba",
    ),
    (
        "phash_rgb8_48x40.png",
        include_bytes!("../fuzz/corpus/png/phash_rgb8_48x40.png"),
        "5ffd68d91cf65683",
    ),
    (
        "phash_rgba8_48x40.png",
        include_bytes!("../fuzz/corpus/png/phash_rgba8_48x40.png"),
        "20f6e523aa5b647b",
    ),
];

/// Default decode ceilings: the corpus holds ordinary photographs-sized
/// seeds, far below both.
fn limits() -> Limits {
    Limits::default()
}

fn digest16(bytes: &[u8]) -> String {
    let d = sha256(bytes).expect("sha256 of a corpus seed");
    d.to_string()[..16].to_string()
}

#[test]
fn every_corpus_seed_decodes_deterministically() {
    assert_eq!(SEEDS.len(), 6, "corpus drift: update PROVENANCE.md too");
    for (name, bytes, pinned) in SEEDS {
        assert_eq!(&digest16(bytes), pinned, "{name}: seed digest drifted");
        let first = decode(bytes, &limits()).unwrap_or_else(|e| panic!("{name}: {e}"));
        let second = decode(bytes, &limits()).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(first, second, "{name}: decode is not deterministic");
        assert_eq!(first.width(), first.pixels().width());
        assert_eq!(first.height(), first.pixels().height());
    }
}
