use aes::cipher::{block_padding::Pkcs7, BlockModeDecrypt, BlockModeEncrypt, KeyIvInit};
use cmac::{Cmac, KeyInit, Mac};
use hmac::Hmac;
use rsa::pkcs1::DecodeRsaPrivateKey;
use rsa::pkcs8::DecodePrivateKey;
use rsa::pss::SigningKey;
use rsa::signature::{RandomizedSigner, SignatureEncoding};
use rsa::{Oaep, RsaPrivateKey, RsaPublicKey};
use sha1::Sha1;
use sha2::Sha256;

use crate::errors::{MhError, MhResult};

type Aes128CbcDec = cbc::Decryptor<aes::Aes128>;
type Aes128CbcEnc = cbc::Encryptor<aes::Aes128>;

pub struct ServiceCertificate {
    provider_id: Vec<u8>,
    serial_number: Vec<u8>,
    public_key: RsaPublicKey,
}

impl ServiceCertificate {}

pub struct WidevineDevice {
    client_id: Vec<u8>,
    rsa_key: RsaPrivateKey,
    is_android: bool,
}

pub struct ContentKey {
    pub key_id: Vec<u8>,
    pub key: Vec<u8>,
    pub key_type: i32,
}

impl WidevineDevice {
    pub fn from_wvd_bytes(data: &[u8]) -> MhResult<Self> {
        if data.len() < 8 || &data[0..3] != b"WVD" {
            return Err(MhError::Parse("not a WVD device file".into()));
        }
        let is_android = data[4] == 2;
        let mut pos = 7usize;
        let read_u16 = |d: &[u8], p: &mut usize| -> MhResult<usize> {
            if *p + 2 > d.len() {
                return Err(MhError::Parse("wvd truncated".into()));
            }
            let v = u16::from_be_bytes([d[*p], d[*p + 1]]) as usize;
            *p += 2;
            Ok(v)
        };
        let pk_len = read_u16(data, &mut pos)?;
        if pos + pk_len > data.len() {
            return Err(MhError::Parse("wvd private key truncated".into()));
        }
        let pk_der = &data[pos..pos + pk_len];
        pos += pk_len;
        let cid_len = read_u16(data, &mut pos)?;
        if pos + cid_len > data.len() {
            return Err(MhError::Parse("wvd client id truncated".into()));
        }
        let client_id = data[pos..pos + cid_len].to_vec();

        let rsa_key = RsaPrivateKey::from_pkcs1_der(pk_der)
            .or_else(|_| RsaPrivateKey::from_pkcs8_der(pk_der))
            .map_err(|e| MhError::Parse(format!("wvd rsa key: {e}")))?;

        Ok(Self {
            client_id,
            rsa_key,
            is_android,
        })
    }

    pub fn is_android(&self) -> bool {
        self.is_android
    }
}

/// Build the WidevinePsshData `pssh_data` init bytes from a raw PSSH.
/// Long input (>30 bytes) is a full pssh box / CencHeader → strip any box header.
/// Short input (≤30 bytes) is a bare key-ID → wrap in WidevinePsshData{algorithm:1, key_ids:[id]}.
pub fn pssh_init_data(pssh_raw: &[u8]) -> Vec<u8> {
    if pssh_raw.len() > 30 {
        if pssh_raw.len() > 32 && &pssh_raw[4..8] == b"pssh" {
            return pssh_raw[32..].to_vec();
        }
        return pssh_raw.to_vec();
    }
    let mut out = Vec::new();
    pb_varint(&mut out, 1, 1);
    pb_bytes(&mut out, 2, pssh_raw);
    out
}

pub fn make_request_id(is_android: bool, counter: u64) -> Vec<u8> {
    if is_android {
        let random4 = rand_16()[..4].to_vec();
        let mut raw = Vec::with_capacity(16);
        raw.extend_from_slice(&random4);
        raw.extend_from_slice(&[0u8; 4]);
        raw.extend_from_slice(&counter.to_le_bytes());
        hex_upper(&raw).into_bytes()
    } else {
        rand_16().to_vec()
    }
}

fn hex_upper(b: &[u8]) -> String {
    let mut s = String::with_capacity(b.len() * 2);
    for x in b {
        s.push_str(&format!("{x:02X}"));
    }
    s
}

pub struct Session {
    request: Vec<u8>,
}

pub fn build_challenge(
    device: &WidevineDevice,
    pssh_init_data: &[u8],
    request_id: &[u8],
    request_time: i64,
    service_cert: Option<&ServiceCertificate>,
) -> MhResult<(Vec<u8>, Session)> {
    let mut wv_pssh = Vec::new();
    pb_bytes(&mut wv_pssh, 1, pssh_init_data);
    pb_varint(&mut wv_pssh, 2, 1);
    pb_bytes(&mut wv_pssh, 3, request_id);
    let mut content_id = Vec::new();
    pb_bytes(&mut content_id, 1, &wv_pssh);

    let mut req = Vec::new();
    match service_cert {
        Some(cert) => {
            let enc = encrypt_client_id(&device.client_id, cert)?;
            pb_bytes(&mut req, 8, &enc);
        }
        None => pb_bytes(&mut req, 1, &device.client_id),
    }
    pb_bytes(&mut req, 2, &content_id);
    pb_varint(&mut req, 3, 1);
    pb_varint(&mut req, 4, request_time as u64);
    pb_varint(&mut req, 6, 21);
    pb_varint(&mut req, 7, (request_time as u64) & 0x7fff_ffff);

    let signing_key = SigningKey::<Sha1>::new(device.rsa_key.clone());
    let signature = signing_key.sign_with_rng(&mut os_rng(), &req).to_vec();

    let mut signed = Vec::new();
    pb_varint(&mut signed, 1, 1);
    pb_bytes(&mut signed, 2, &req);
    pb_bytes(&mut signed, 3, &signature);

    Ok((signed, Session { request: req }))
}

fn encrypt_client_id(client_id: &[u8], cert: &ServiceCertificate) -> MhResult<Vec<u8>> {
    let privacy_key: [u8; 16] = rand_16();
    let privacy_iv: [u8; 16] = rand_16();

    let enc_cid = Aes128CbcEnc::new_from_slices(&privacy_key, &privacy_iv)
        .map_err(|e| MhError::Other(format!("privacy aes: {e}")))?
        .encrypt_padded_vec::<Pkcs7>(client_id);

    let enc_key = cert
        .public_key
        .encrypt(&mut os_rng(), Oaep::<Sha1>::new(), &privacy_key)
        .map_err(|e| MhError::Other(format!("privacy key oaep: {e}")))?;

    let mut out = Vec::new();
    if !cert.provider_id.is_empty() {
        pb_bytes(&mut out, 1, &cert.provider_id);
    }
    if !cert.serial_number.is_empty() {
        pb_bytes(&mut out, 2, &cert.serial_number);
    }
    pb_bytes(&mut out, 3, &enc_cid);
    pb_bytes(&mut out, 4, &privacy_iv);
    pb_bytes(&mut out, 5, &enc_key);
    Ok(out)
}

fn os_rng() -> impl rsa::rand_core::CryptoRng {
    rsa::rand_core::UnwrapErr(getrandom::SysRng)
}

fn rand_16() -> [u8; 16] {
    use rsa::rand_core::Rng;
    let mut b = [0u8; 16];
    os_rng().fill_bytes(&mut b);
    b
}

pub fn parse_license(
    device: &WidevineDevice,
    session: &Session,
    license_message: &[u8],
) -> MhResult<Vec<ContentKey>> {
    let sm = PbReader::new(license_message);
    let mut license_bytes: Vec<u8> = Vec::new();
    let mut session_key: Vec<u8> = Vec::new();
    let mut signature: Vec<u8> = Vec::new();
    let mut oemcrypto: Vec<u8> = Vec::new();
    for (field, val) in sm {
        match (field.0, val) {
            (2, PbVal::Bytes(b)) => license_bytes = b.to_vec(),
            (3, PbVal::Bytes(b)) => signature = b.to_vec(),
            (4, PbVal::Bytes(b)) => session_key = b.to_vec(),
            (6, PbVal::Bytes(b)) => oemcrypto = b.to_vec(),
            _ => {}
        }
    }
    if license_bytes.is_empty() || session_key.is_empty() {
        return Err(MhError::Other(
            "widevine license missing msg/session_key".into(),
        ));
    }

    let derived_key = device
        .rsa_key
        .decrypt(Oaep::<Sha1>::new(), &session_key)
        .map_err(|e| MhError::Other(format!("widevine session_key decrypt: {e}")))?;

    let enc_context = context_bytes(b"ENCRYPTION", &session.request, 128);
    let mac_context = context_bytes(b"AUTHENTICATION", &session.request, 512);
    let enc_key = cmac_derive(&derived_key, &enc_context, 1)?;
    let mut mac_key_server = cmac_derive(&derived_key, &mac_context, 1)?;
    mac_key_server.extend(cmac_derive(&derived_key, &mac_context, 2)?);

    let mut mac = Hmac::<Sha256>::new_from_slice(&mac_key_server)
        .map_err(|e| MhError::Other(format!("widevine hmac init: {e}")))?;
    mac.update(&oemcrypto);
    mac.update(&license_bytes);
    let computed = mac.finalize().into_bytes();
    if computed.as_slice() != signature.as_slice() {
        return Err(MhError::Other("widevine license signature mismatch".into()));
    }

    let mut keys = Vec::new();
    for (field, val) in PbReader::new(&license_bytes) {
        if let (3, PbVal::Bytes(kc)) = (field.0, val) {
            if let Some(k) = parse_key_container(kc, &enc_key)? {
                keys.push(k);
            }
        }
    }
    if keys.is_empty() {
        return Err(MhError::Other(
            "widevine license had no content keys".into(),
        ));
    }
    Ok(keys)
}

fn parse_key_container(data: &[u8], enc_key: &[u8]) -> MhResult<Option<ContentKey>> {
    let mut id = Vec::new();
    let mut iv = Vec::new();
    let mut enc = Vec::new();
    let mut ktype = 0i32;
    for (field, val) in PbReader::new(data) {
        match (field.0, val) {
            (1, PbVal::Bytes(b)) => id = b.to_vec(),
            (2, PbVal::Bytes(b)) => iv = b.to_vec(),
            (3, PbVal::Bytes(b)) => enc = b.to_vec(),
            (4, PbVal::Varint(v)) => ktype = v as i32,
            _ => {}
        }
    }
    if iv.len() != 16 || enc.is_empty() {
        return Ok(None);
    }
    let dec = Aes128CbcDec::new_from_slices(enc_key, &iv)
        .map_err(|e| MhError::Other(format!("widevine key cbc: {e}")))?;
    let key = dec
        .decrypt_padded_vec::<Pkcs7>(&enc)
        .map_err(|e| MhError::Other(format!("widevine key unpad: {e}")))?;
    Ok(Some(ContentKey {
        key_id: id,
        key,
        key_type: ktype,
    }))
}

fn context_bytes(label: &[u8], message: &[u8], key_bits: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(label.len() + 1 + message.len() + 4);
    out.extend_from_slice(label);
    out.push(0);
    out.extend_from_slice(message);
    out.extend_from_slice(&key_bits.to_be_bytes());
    out
}

fn cmac_derive(session_key: &[u8], context: &[u8], counter: u8) -> MhResult<Vec<u8>> {
    let mut mac = Cmac::<aes::Aes128>::new_from_slice(session_key)
        .map_err(|e| MhError::Other(format!("widevine cmac init: {e}")))?;
    mac.update(&[counter]);
    mac.update(context);
    Ok(mac.finalize().into_bytes().to_vec())
}

fn pb_key(out: &mut Vec<u8>, field: u32, wire: u8) {
    write_varint(out, ((field << 3) | wire as u32) as u64);
}
fn pb_varint(out: &mut Vec<u8>, field: u32, v: u64) {
    pb_key(out, field, 0);
    write_varint(out, v);
}
fn pb_bytes(out: &mut Vec<u8>, field: u32, b: &[u8]) {
    pb_key(out, field, 2);
    write_varint(out, b.len() as u64);
    out.extend_from_slice(b);
}
fn write_varint(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let mut b = (v & 0x7f) as u8;
        v >>= 7;
        if v != 0 {
            b |= 0x80;
        }
        out.push(b);
        if v == 0 {
            break;
        }
    }
}

enum PbVal<'a> {
    Varint(u64),
    Bytes(&'a [u8]),
}
struct PbReader<'a> {
    data: &'a [u8],
    pos: usize,
}
impl<'a> PbReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }
}
impl<'a> Iterator for PbReader<'a> {
    type Item = ((u32, u8), PbVal<'a>);
    fn next(&mut self) -> Option<Self::Item> {
        if self.pos >= self.data.len() {
            return None;
        }
        let tag = read_varint(self.data, &mut self.pos)?;
        let field = (tag >> 3) as u32;
        let wire = (tag & 7) as u8;
        match wire {
            0 => {
                let v = read_varint(self.data, &mut self.pos)?;
                Some(((field, wire), PbVal::Varint(v)))
            }
            2 => {
                let len = read_varint(self.data, &mut self.pos)? as usize;
                if self.pos + len > self.data.len() {
                    return None;
                }
                let b = &self.data[self.pos..self.pos + len];
                self.pos += len;
                Some(((field, wire), PbVal::Bytes(b)))
            }
            5 => {
                self.pos += 4;
                self.next()
            }
            1 => {
                self.pos += 8;
                self.next()
            }
            _ => None,
        }
    }
}
fn read_varint(data: &[u8], pos: &mut usize) -> Option<u64> {
    let mut v = 0u64;
    let mut shift = 0u32;
    loop {
        if *pos >= data.len() || shift >= 64 {
            return None;
        }
        let b = data[*pos];
        *pos += 1;
        v |= ((b & 0x7f) as u64) << shift;
        if b & 0x80 == 0 {
            return Some(v);
        }
        shift += 7;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varint_roundtrip() {
        let mut buf = Vec::new();
        write_varint(&mut buf, 300);
        let mut pos = 0;
        assert_eq!(read_varint(&buf, &mut pos), Some(300));
    }

    #[test]
    fn pb_bytes_reads_back() {
        let mut buf = Vec::new();
        pb_bytes(&mut buf, 2, b"hello");
        let mut got = None;
        for (f, v) in PbReader::new(&buf) {
            if let (2, PbVal::Bytes(b)) = (f.0, v) {
                got = Some(b.to_vec());
            }
        }
        assert_eq!(got.as_deref(), Some(&b"hello"[..]));
    }

    #[test]
    fn context_has_label_and_bits() {
        let c = context_bytes(b"ENCRYPTION", b"msg", 128);
        assert!(c.starts_with(b"ENCRYPTION\x00msg"));
        assert_eq!(&c[c.len() - 4..], &128u32.to_be_bytes());
    }
}
