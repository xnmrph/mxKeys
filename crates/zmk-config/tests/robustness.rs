//! The parser must never panic and must stay lossless on arbitrary (mangled) input.

use zmk_config::dts::Document;
use zmk_config::keymap::Keymap;

/// Small deterministic xorshift PRNG so the test needs no extra dependencies.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

const INTERESTING: &[&str] = &[
    "{",
    "}",
    "<",
    ">",
    ";",
    "&",
    "(",
    ")",
    "\"",
    "/*",
    "*/",
    "//",
    "#define X ",
    "\n",
    "\\\n",
    "=",
    ",",
    "/delete-node/",
    "@",
    "ü",
    "\r\n",
    "'",
];

#[test]
fn mutated_keymaps_never_panic_and_round_trip() {
    let base = [
        include_str!("fixtures/corne_stock.keymap"),
        include_str!("fixtures/advanced.keymap"),
        include_str!("fixtures/helper_macros.keymap"),
    ];
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for round in 0..600 {
        let mut s = base[round % base.len()].to_string();
        for _ in 0..(1 + rng.below(6)) {
            let mut at = rng.below(s.len() + 1);
            while !s.is_char_boundary(at) {
                at -= 1;
            }
            match rng.below(3) {
                0 => s.insert_str(at, INTERESTING[rng.below(INTERESTING.len())]),
                1 => {
                    let mut end = (at + rng.below(40)).min(s.len());
                    while !s.is_char_boundary(end) {
                        end -= 1;
                    }
                    s.replace_range(at..end, "");
                }
                _ => s.truncate(at),
            }
        }
        let doc = Document::parse(s.clone());
        assert_eq!(doc.to_source(), s, "round {round}");
        let _ = Keymap::from_document(&doc);
    }
}
