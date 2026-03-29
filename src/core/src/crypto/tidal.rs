use aes::Aes128;
use cbc::cipher::{BlockDecryptMut, KeyIvInit as CbcKeyIvInit};
use ctr::cipher::StreamCipher;
#[allow(unused_imports)]
use ctr::cipher::KeyIvInit as CtrKeyIvInit;

pub const TIDAL_MASTER_KEY: &[u8] = &[
    0x50, 0x89, 0x53, 0x4c, 0x43, 0x26, 0x98, 0xb7,
    0x82, 0x8a, 0x2c, 0x6f, 0xd1, 0xb6, 0xa4, 0xc7,
    0x61, 0xf8, 0xe5, 0x6e, 0x8c, 0x74, 0x68, 0x13,
    0x45, 0xfa, 0x3f, 0xba, 0x68, 0x38, 0xe7, 0xae,
];

type Aes128Cbc = cbc::Decryptor<Aes128>;
type Aes128Ctr = ctr::Ctr64BE<Aes128>;

pub fn decrypt_mqa(encrypted_data: &[u8], encryption_key: &str) -> Vec<u8> {
    use base64::Engine;
    let security_token = base64::engine::general_purpose::STANDARD
        .decode(encryption_key)
        .expect("valid base64 encryption_key");

    let iv = &security_token[..16];
    let encrypted_st = &security_token[16..];

    let mut buf = encrypted_st.to_vec();
    let pad = (16 - buf.len() % 16) % 16;
    buf.extend(std::iter::repeat(0u8).take(pad));

    let cipher = Aes128Cbc::new_from_slices(TIDAL_MASTER_KEY, iv)
        .expect("valid key/iv for tidal master");
    use cbc::cipher::block_padding::NoPadding;
    let _ = cipher.decrypt_padded_mut::<NoPadding>(&mut buf);
    let decrypted_st = buf.to_vec();

    let key = &decrypted_st[..16];
    let nonce = &decrypted_st[16..24];

    let mut ctr_iv = [0u8; 16];
    ctr_iv[..8].copy_from_slice(nonce);

    let mut out = encrypted_data.to_vec();
    let mut cipher = Aes128Ctr::new_from_slices(key, &ctr_iv)
        .expect("valid key/iv for tidal ctr");
    cipher.apply_keystream(&mut out);
    out
}
