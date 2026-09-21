use futures_util::StreamExt;
use std::path::Path;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

use crate::errors::{MhError, MhResult};
use crate::http_client::{is_transient_http_error, retry_transient};
use crate::services::common::pipeline::downloader::part_path;

use aes::Aes128;
use blowfish::Blowfish;
use cbc::cipher::{BlockModeDecrypt, BlockModeEncrypt, KeyIvInit};
use ecb::cipher::KeyInit;
use md5::{Digest, Md5};

const BLOWFISH_SECRET: &[u8] = b"g4el58wc0zvf9na1";

pub const DEEZER_FORMAT_NUMBERS: [(u8, u8); 3] = [(0, 9), (1, 3), (2, 1)];

pub fn generate_blowfish_key(track_id: &str) -> [u8; 16] {
    let mut hasher = Md5::new();
    hasher.update(track_id.as_bytes());
    let hash = hasher.finalize();
    let hex = hex::encode(hash);
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
    buf.extend(std::iter::repeat_n(0u8, pad));

    let cipher = BlowfishCbc::new_from_slices(key, &iv).expect("blowfish key/iv size is valid");
    use cbc::cipher::block_padding::NoPadding;
    let _ = cipher.decrypt_padded::<NoPadding>(&mut buf);
    buf[..block.len().min(buf.len())].to_vec()
}

pub fn decrypt_buffer(track_id: &str, buf: &[u8]) -> Vec<u8> {
    decrypt_stripe(&generate_blowfish_key(track_id), buf)
}

/// Deezer encrypts only the first 2048 bytes of every 6144-byte stripe, and each
/// stripe is independent — which is what lets a download be decrypted as it
/// arrives instead of being held in memory until the last byte.
pub const DEEZER_STRIPE: usize = 6144;

pub fn decrypt_stripe(key: &[u8; 16], buf: &[u8]) -> Vec<u8> {
    const CHUNK: usize = DEEZER_STRIPE;
    const ENCRYPTED: usize = 2048;

    let mut out = vec![0u8; buf.len()];
    let mut pos = 0usize;

    while pos < buf.len() {
        let remaining = buf.len() - pos;
        if remaining >= ENCRYPTED {
            let decrypted = decrypt_chunk(key, &buf[pos..pos + ENCRYPTED]);
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
    let url_hash_hex = hex::encode(url_hash_bytes);

    let mut info_bytes: Vec<u8> = Vec::new();
    info_bytes.extend_from_slice(url_hash_hex.as_bytes());
    info_bytes.push(SEP);
    info_bytes.extend_from_slice(&url_bytes);
    info_bytes.push(SEP);

    let remainder = info_bytes.len() % 16;
    if remainder != 0 {
        let padding = 16 - remainder;
        info_bytes.extend(std::iter::repeat_n(0x2eu8, padding));
    }

    let aes_key = b"jo6aey6haid2Teih";
    use ecb::cipher::block_padding::NoPadding;
    let cipher = Aes128Ecb::new_from_slice(aes_key).expect("valid AES key");
    let encrypted = cipher.encrypt_padded_vec::<NoPadding>(&info_bytes);

    let hex_path = hex::encode(&encrypted);

    let first_char = md5_origin.chars().next().unwrap_or('a');
    format!(
        "https://e-cdns-proxy-{}.dzcdn.net/mobile/1/{}",
        first_char, hex_path
    )
}

/// Whether the first bytes of a decrypted file are the container the extension claims.
/// A wrong Blowfish key yields a full-length file of noise, which every later stage
/// mistakes for a healthy download until lofty refuses it at tagging time.
fn decrypted_magic_ok(head: &[u8], ext: &str) -> bool {
    match ext {
        "flac" => head.starts_with(b"fLaC"),
        "mp3" => {
            head.starts_with(b"ID3")
                || (head.len() >= 2 && head[0] == 0xFF && (head[1] & 0xE0) == 0xE0)
        }
        _ => true,
    }
}

/// The Deezer stream, decrypted onto disk, retried on a dropped connection.
///
/// `download_file` has always retried transient failures; this path is a separate
/// implementation because the body has to be Blowfish-decrypted stripe by stripe as it
/// arrives, and it never picked the retry up. A cipher mismatch is deliberately *not*
/// transient — it produces `MhError::Other`, so re-fetching cannot help and does not
/// happen.
pub async fn download_and_decrypt_deezer(
    client: &reqwest::Client,
    url: &str,
    crypto_id: &str,
    ext: &str,
    dest: &Path,
    known_size: Option<u64>,
    on_progress: impl Fn(u64, u64),
) -> MhResult<()> {
    const MAX_ATTEMPTS: u32 = 3;
    let is_transient = |e: &MhError| matches!(e, MhError::Network(_)) || is_transient_http_error(e);
    retry_transient(MAX_ATTEMPTS, is_transient, || {
        decrypt_deezer_once(client, url, crypto_id, ext, dest, known_size, &on_progress)
    })
    .await
}

async fn decrypt_deezer_once(
    client: &reqwest::Client,
    url: &str,
    crypto_id: &str,
    ext: &str,
    dest: &Path,
    known_size: Option<u64>,
    on_progress: impl Fn(u64, u64),
) -> MhResult<()> {
    let resp = client.get(url).send().await?;
    if !resp.status().is_success() {
        return Err(MhError::Other(format!(
            "HTTP {} for Deezer stream",
            resp.status().as_u16()
        )));
    }

    let total = known_size
        .filter(|n| *n > 0)
        .or_else(|| resp.content_length())
        .unwrap_or(0);
    let mut stream = resp.bytes_stream();
    let part = part_path(dest);
    let mut file = File::create(&part).await?;
    let key = generate_blowfish_key(crypto_id);
    let mut pending: Vec<u8> = Vec::with_capacity(DEEZER_STRIPE * 2);
    let mut downloaded: u64 = 0;

    while let Some(chunk) = stream.next().await {
        let chunk = match chunk {
            Ok(c) => c,
            Err(e) => {
                let _ = tokio::fs::remove_file(&part).await;
                return Err(e.into());
            }
        };
        downloaded += chunk.len() as u64;
        pending.extend_from_slice(&chunk);

        let whole = pending.len() - (pending.len() % DEEZER_STRIPE);
        if whole > 0 {
            file.write_all(&decrypt_stripe(&key, &pending[..whole]))
                .await?;
            pending.drain(..whole);
        }
        on_progress(downloaded, total);
    }

    if !pending.is_empty() {
        file.write_all(&decrypt_stripe(&key, &pending)).await?;
    }
    file.flush().await?;
    drop(file);

    let mut head = [0u8; 4];
    let read = {
        use tokio::io::AsyncReadExt;
        let mut f = File::open(&part).await?;
        f.read(&mut head).await.unwrap_or(0)
    };
    if !decrypted_magic_ok(&head[..read], ext) {
        let _ = tokio::fs::remove_file(&part).await;
        return Err(MhError::Other(format!(
            "Deezer stream for song {crypto_id} did not decrypt to {ext}: the file starts \
             {head:02x?}, not the {ext} magic. Deezer served this track's media under a \
             different song id or cipher than the one the key was derived from."
        )));
    }

    tokio::fs::rename(&part, dest).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The streaming download decrypts each completed 6144-byte stripe as it lands
    /// and holds only the ragged tail. That is only safe if splitting on a stripe
    /// boundary produces the same bytes as decrypting the whole file at once.
    #[test]
    fn splitting_on_a_stripe_boundary_changes_nothing() {
        let key = generate_blowfish_key("3135556");
        for len in [
            1,
            2047,
            2048,
            DEEZER_STRIPE - 1,
            DEEZER_STRIPE,
            DEEZER_STRIPE + 1,
            DEEZER_STRIPE * 3 + 517,
        ] {
            let buf: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
            let whole = decrypt_stripe(&key, &buf);

            let split = len - (len % DEEZER_STRIPE);
            let mut streamed = decrypt_stripe(&key, &buf[..split]);
            if split < len {
                streamed.extend(decrypt_stripe(&key, &buf[split..]));
            }
            assert_eq!(streamed, whole, "len {len}");
        }
    }

    #[test]
    fn a_key_is_derived_from_the_track_id() {
        assert_ne!(
            generate_blowfish_key("3135556"),
            generate_blowfish_key("3135557")
        );
    }
}
