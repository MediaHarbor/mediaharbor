use aes::Aes128;
use ctr::cipher::{KeyIvInit, StreamCipher};

pub const SPOTIFY_IV: [u8; 16] = [
    72, 116, 29, 14, 199, 81, 90, 66,
    130, 143, 22, 46, 55, 134, 44, 188,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioFormat {
    Ogg,
    Mp3,
    Flac,
    Unknown,
}

type Aes128Ctr = ctr::Ctr128BE<Aes128>;

pub fn decrypt_stream(encrypted: &[u8], key: &[u8; 16]) -> Vec<u8> {
    let mut out = encrypted.to_vec();
    let mut cipher = Aes128Ctr::new_from_slices(key, &SPOTIFY_IV)
        .expect("valid key/iv for spotify ctr");
    cipher.apply_keystream(&mut out);
    out
}

const OGG_MAGIC: &[u8] = b"OggS";

pub fn strip_ogg_header(data: &[u8]) -> &[u8] {
    if data.starts_with(OGG_MAGIC) && data.len() > 167 {
        &data[167..]
    } else {
        data
    }
}

pub fn detect_format(data: &[u8]) -> AudioFormat {
    if data.starts_with(OGG_MAGIC) {
        AudioFormat::Ogg
    } else if data.starts_with(b"ID3") || (data.len() >= 2 && data[0] == 0xFF && (data[1] & 0xE0) == 0xE0) {
        AudioFormat::Mp3
    } else if data.starts_with(b"fLaC") {
        AudioFormat::Flac
    } else {
        AudioFormat::Unknown
    }
}
