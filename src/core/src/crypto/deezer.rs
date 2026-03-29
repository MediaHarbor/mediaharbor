use aes::Aes128;
use blowfish::Blowfish;
use cbc::cipher::{BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use ecb::cipher::KeyInit;
use md5::{Digest, Md5};

const BLOWFISH_SECRET: &[u8] = b"g4el58wc0zvf9na1";

pub const DEEZER_FORMAT_NUMBERS: [(u8, u8); 3] = [(0, 9), (1, 3), (2, 1)];

pub fn generate_blowfish_key(track_id: &str) -> [u8; 16] {
    let mut hasher = Md5::new();
    hasher.update(track_id.as_bytes());
    let hash = hasher.finalize();
    let hex = format!("{:x}", hash);
    let hex_bytes = hex.as_bytes();

    let mut key = [0u8; 16];
    for i in 0..16 {
        key[i] = hex_bytes[i] ^ hex_bytes[i + 16] ^ BLOWFISH_SECRET[i];
    }
    key
}

type BlowfishCbc = cbc::Decryptor<Blowfish>;

pub fn decrypt_chunk(key: &[u8; 16], block: &[u8]) -> Vec<u8> {
    let iv = [0u8, 1, 2, 3, 4, 5, 6, 7];
    let mut buf = block.to_vec();
    let pad = (8 - buf.len() % 8) % 8;
    buf.extend(std::iter::repeat(0u8).take(pad));

    let cipher = BlowfishCbc::new_from_slices(key, &iv)
        .expect("blowfish key/iv size is valid");
    use cbc::cipher::block_padding::NoPadding;
    let _ = cipher.decrypt_padded_mut::<NoPadding>(&mut buf);
    buf[..block.len().min(buf.len())].to_vec()
}

pub fn decrypt_buffer(track_id: &str, buf: &[u8]) -> Vec<u8> {
    let key = generate_blowfish_key(track_id);
    const CHUNK: usize = 6144;
    const ENCRYPTED: usize = 2048;

    let mut out = vec![0u8; buf.len()];
    let mut pos = 0usize;

    while pos < buf.len() {
        let remaining = buf.len() - pos;
        if remaining >= ENCRYPTED {
            let decrypted = decrypt_chunk(&key, &buf[pos..pos + ENCRYPTED]);
            let copy_len = decrypted.len().min(ENCRYPTED);
            out[pos..pos + copy_len].copy_from_slice(&decrypted[..copy_len]);

            let plain_len = (remaining - ENCRYPTED).min(CHUNK - ENCRYPTED);
            if plain_len > 0 {
                out[pos + ENCRYPTED..pos + ENCRYPTED + plain_len]
                    .copy_from_slice(&buf[pos + ENCRYPTED..pos + ENCRYPTED + plain_len]);
            }
            pos += remaining.min(CHUNK);
        } else {
            out[pos..pos + remaining].copy_from_slice(&buf[pos..pos + remaining]);
            pos += remaining;
        }
    }

    out[..buf.len()].to_vec()
}

type Aes128Ecb = ecb::Encryptor<Aes128>;

pub fn get_encrypted_url(
    track_id: &str,
    md5_origin: &str,
    media_version: &str,
    quality: u8,
) -> String {
    let format_number = DEEZER_FORMAT_NUMBERS
        .iter()
        .find(|(q, _)| *q == quality)
        .map(|(_, n)| *n)
        .unwrap_or(3u8);

    const SEP: u8 = 0xa4;

    let mut url_bytes: Vec<u8> = Vec::new();
    url_bytes.extend_from_slice(md5_origin.as_bytes());
    url_bytes.push(SEP);
    url_bytes.extend_from_slice(format_number.to_string().as_bytes());
    url_bytes.push(SEP);
    url_bytes.extend_from_slice(track_id.as_bytes());
    url_bytes.push(SEP);
    url_bytes.extend_from_slice(media_version.as_bytes());

    let mut hasher = Md5::new();
    hasher.update(&url_bytes);
    let url_hash_bytes = hasher.finalize();
    let url_hash_hex = format!("{:x}", url_hash_bytes);

    let mut info_bytes: Vec<u8> = Vec::new();
    info_bytes.extend_from_slice(url_hash_hex.as_bytes());
    info_bytes.push(SEP);
    info_bytes.extend_from_slice(&url_bytes);
    info_bytes.push(SEP);

    let remainder = info_bytes.len() % 16;
    if remainder != 0 {
        let padding = 16 - remainder;
        info_bytes.extend(std::iter::repeat(0x2eu8).take(padding));
    }

    let aes_key = b"jo6aey6haid2Teih";
    use ecb::cipher::block_padding::NoPadding;
    let cipher = Aes128Ecb::new_from_slice(aes_key).expect("valid AES key");
    let encrypted = cipher
        .encrypt_padded_vec_mut::<NoPadding>(&info_bytes);

    let hex_path = hex::encode(&encrypted);

    let first_char = md5_origin.chars().next().unwrap_or('a');
    format!(
        "https://e-cdns-proxy-{}.dzcdn.net/mobile/1/{}",
        first_char, hex_path
    )
}
