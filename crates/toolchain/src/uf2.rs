//! Minimal UF2 inspection: is a file a well-formed UF2, and for which chip family?
//! (Phase 4's flashing uses this to refuse mismatched firmware.)

use serde::Serialize;

const MAGIC_START0: u32 = 0x0A32_4655; // "UF2\n"
const MAGIC_START1: u32 = 0x9E5D_5157;
const MAGIC_END: u32 = 0x0AB1_6F30;
const FLAG_FAMILY_ID: u32 = 0x0000_2000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Uf2Summary {
    pub blocks: usize,
    /// Family id from the first block (e.g. 0xADA52840 = nRF52840), if present.
    pub family: Option<u32>,
    /// Lowest target address.
    pub start_address: u32,
    /// Payload bytes.
    pub payload: u64,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Uf2Error {
    #[error("not a UF2 file (size {0} isn't a multiple of 512)")]
    Size(usize),
    #[error("not a UF2 file (bad magic in block {0})")]
    Magic(usize),
    #[error("UF2 file is empty")]
    Empty,
}

fn word(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

pub fn inspect(data: &[u8]) -> Result<Uf2Summary, Uf2Error> {
    if data.is_empty() {
        return Err(Uf2Error::Empty);
    }
    if data.len() % 512 != 0 {
        return Err(Uf2Error::Size(data.len()));
    }
    let mut family = None;
    let mut start = u32::MAX;
    let mut payload = 0u64;
    for (n, block) in data.chunks(512).enumerate() {
        if word(block, 0) != MAGIC_START0 || word(block, 4) != MAGIC_START1 || word(block, 508) != MAGIC_END {
            return Err(Uf2Error::Magic(n));
        }
        let flags = word(block, 8);
        if n == 0 && flags & FLAG_FAMILY_ID != 0 {
            family = Some(word(block, 28));
        }
        start = start.min(word(block, 12));
        payload += u64::from(word(block, 16));
    }
    Ok(Uf2Summary {
        blocks: data.len() / 512,
        family,
        start_address: start,
        payload,
    })
}

#[cfg(test)]
pub(crate) fn block(addr: u32, family: u32) -> Vec<u8> {
    let mut b = vec![0u8; 512];
    for (i, w) in [
        MAGIC_START0,
        MAGIC_START1,
        FLAG_FAMILY_ID,
        addr,
        256,
        0,
        1,
        family,
    ]
    .iter()
    .enumerate()
    {
        b[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
    }
    b[508..].copy_from_slice(&MAGIC_END.to_le_bytes());
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_and_invalid() {
        let mut f = block(0x26000, 0xADA5_2840);
        f.extend(block(0x26100, 0xADA5_2840));
        let s = inspect(&f).unwrap();
        assert_eq!(
            (s.blocks, s.family, s.start_address, s.payload),
            (2, Some(0xADA5_2840), 0x26000, 512)
        );
        assert_eq!(inspect(&[]), Err(Uf2Error::Empty));
        assert_eq!(inspect(&f[..700]), Err(Uf2Error::Size(700)));
        let mut bad = f.clone();
        bad[512] = 0;
        assert_eq!(inspect(&bad), Err(Uf2Error::Magic(1)));
    }
}
