
use aes::Aes128;
use cbc::cipher::{BlockDecryptMut, KeyIvInit as CbcKeyIvInit};
use cbc::cipher::block_padding::NoPadding;
#[allow(unused_imports)]
use ctr::cipher::{KeyIvInit as CtrKeyIvInit, StreamCipher};

use crate::errors::{MhError, MhResult};

pub const DEFAULT_SONG_DECRYPTION_KEY: &str = "32b8ade1769e26b1ffb8986352793fc6";

fn is_container_type(t: &str) -> bool {
    matches!(
        t,
        "moov" | "trak" | "mdia" | "minf" | "stbl" | "udta" | "mvex"
            | "moof" | "traf" | "sinf" | "schi"
    )
}

#[derive(Debug, Clone)]
pub struct Mp4Box {
    pub box_type: String,
    pub data: Vec<u8>,
    pub children: Vec<Mp4Box>,
}

#[derive(Debug, Clone)]
pub struct Fragment {
    pub moof: Vec<u8>,
    pub mdat: Vec<u8>,
    pub samples: Vec<Sample>,
}

#[derive(Debug, Clone)]
pub struct Sample {
    pub data: Vec<u8>,
    pub iv: Vec<u8>,
    pub subsamples: Vec<(u16, u32)>,
    pub duration: u32,
    pub desc_index: usize,
}

#[derive(Debug, Clone, Default)]
pub struct TrexDefaults {
    pub default_sample_flags: u32,
    pub default_sample_duration: u32,
    pub default_sample_size: u32,
    pub default_sample_desc_index: u32,
}

#[derive(Debug, Clone, Default)]
pub struct TfhdInfo {
    pub track_id: u32,
    pub desc_index: Option<u32>,
    pub base_data_offset: Option<u64>,
    pub default_sample_flags: Option<u32>,
    pub default_sample_duration: Option<u32>,
    pub default_sample_size: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct TrunSample {
    pub duration: Option<u32>,
    pub size: Option<u32>,
    pub flags: Option<u32>,
    pub ct_offset: Option<i32>,
}

#[derive(Debug, Clone)]
pub struct SencEntry {
    pub iv: Vec<u8>,
    pub subsamples: Vec<(u16, u32)>,
}

#[derive(Debug, Clone)]
pub struct EncInfo {
    pub scheme_type: String,
    pub key_id: Vec<u8>,
    pub iv_size: u8,
    pub per_sample_iv_size: u8,
    pub constant_iv: Option<Vec<u8>>,
}

pub struct BoxTransforms {
    pub stsd: Option<fn(&[u8]) -> Vec<u8>>,
}

pub struct DecryptState {
    pub track_id: u32,
    pub enc_infos: Vec<EncInfo>,
    pub trex_defaults: TrexDefaults,
    pub fragments: Vec<Fragment>,
    pub moov: Vec<u8>,
    keys: [Option<Vec<u8>>; 2], // keys[0] = default key, keys[1] = track key
    per_sample_iv_size: u8,
    enc_info_per_desc: Option<std::collections::HashMap<usize, EncInfoDesc>>,
    pair_count: usize,
    pub header: Vec<u8>,
}

#[derive(Debug, Clone)]
struct EncInfoDesc {
    scheme_type: String,
    per_sample_iv_size: u8,
    constant_iv: Option<Vec<u8>>,
}

pub fn read_boxes(data: &[u8]) -> Vec<(String, usize, usize, usize)> {
    read_boxes_range(data, 0, data.len())
}

fn read_boxes_range(data: &[u8], start: usize, end: usize) -> Vec<(String, usize, usize, usize)> {
    let mut boxes = Vec::new();
    let mut off = start;
    while off + 8 <= end {
        let size32 = u32::from_be_bytes([data[off], data[off+1], data[off+2], data[off+3]]) as usize;
        if size32 == 0 {
            break;
        }
        let (size, hdr_size) = if size32 == 1 {
            if off + 16 > end {
                break;
            }
            let hi = u32::from_be_bytes([data[off+8], data[off+9], data[off+10], data[off+11]]) as u64;
            let lo = u32::from_be_bytes([data[off+12], data[off+13], data[off+14], data[off+15]]) as u64;
            let s = (hi * 0x100000000 + lo) as usize;
            (s, 16usize)
        } else {
            (size32, 8usize)
        };
        if size < hdr_size || off + size > end {
            break;
        }
        let box_type = std::str::from_utf8(&data[off+4..off+8])
            .unwrap_or("????")
            .to_string();
        boxes.push((box_type, off, size, hdr_size));
        off += size;
    }
    boxes
}

pub fn find_child_box<'a>(data: &'a [u8], box_type: &str) -> Option<(String, usize, usize, usize)> {
    find_child_box_skip(data, box_type, 8)
}

fn find_child_box_skip(data: &[u8], box_type: &str, skip: usize) -> Option<(String, usize, usize, usize)> {
    let boxes = read_boxes_range(data, skip, data.len());
    boxes.into_iter().find(|(t, _, _, _)| t == box_type)
}

fn read_u32be(data: &[u8], off: usize) -> u32 {
    u32::from_be_bytes([data[off], data[off+1], data[off+2], data[off+3]])
}

fn read_u16be(data: &[u8], off: usize) -> u16 {
    u16::from_be_bytes([data[off], data[off+1]])
}

fn read_i32be(data: &[u8], off: usize) -> i32 {
    i32::from_be_bytes([data[off], data[off+1], data[off+2], data[off+3]])
}

pub fn find_original_format(entry_data: &[u8]) -> Option<Vec<u8>> {
    let sinf_marker = b"sinf";
    let sinf_idx = find_bytes(entry_data, sinf_marker)?;
    if sinf_idx < 4 {
        return None;
    }
    let sinf_start = sinf_idx - 4;
    let sinf_size = read_u32be(entry_data, sinf_start) as usize;
    if sinf_size < 16 || sinf_start + sinf_size > entry_data.len() {
        return None;
    }
    let sinf = &entry_data[sinf_start..sinf_start + sinf_size];

    let frma_marker = b"frma";
    let frma_idx = find_bytes(sinf, frma_marker)?;
    if frma_idx < 4 {
        return None;
    }
    let frma_start = frma_idx - 4;
    let frma_size = read_u32be(sinf, frma_start) as usize;
    if frma_size != 12 || frma_start + 12 > sinf.len() {
        return None;
    }
    Some(sinf[frma_idx + 4..frma_idx + 8].to_vec())
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

pub fn clean_stsd_entry(entry_data: &[u8]) -> Vec<u8> {
    if entry_data.len() < 8 {
        return entry_data.to_vec();
    }
    let entry_type = std::str::from_utf8(&entry_data[4..8]).unwrap_or("????");
    let is_encrypted = matches!(entry_type, "enca" | "encv" | "encs" | "encm");

    let has_sinf = find_bytes(entry_data, b"sinf").is_some();
    if !is_encrypted && !has_sinf {
        return entry_data.to_vec();
    }

    let original_format = find_original_format(entry_data)
        .unwrap_or_else(|| {
            if is_encrypted {
                b"mp4a".to_vec()
            } else {
                entry_type.as_bytes().to_vec()
            }
        });

    const FIXED_HDR: usize = 36;
    if entry_data.len() < FIXED_HDR {
        return entry_data.to_vec();
    }

    let mut new_hdr = entry_data[..FIXED_HDR].to_vec();
    let fmt_len = original_format.len().min(4);
    new_hdr[4..4 + fmt_len].copy_from_slice(&original_format[..fmt_len]);

    let mut children: Vec<Vec<u8>> = Vec::new();
    let mut off = FIXED_HDR;
    while off + 8 <= entry_data.len() {
        let sz = read_u32be(entry_data, off) as usize;
        let ct = std::str::from_utf8(&entry_data[off+4..off+8]).unwrap_or("????");
        if sz < 8 || off + sz > entry_data.len() {
            break;
        }
        if ct != "sinf" {
            children.push(entry_data[off..off + sz].to_vec());
        }
        off += sz;
    }

    let mut body: Vec<u8> = new_hdr;
    for c in children {
        body.extend_from_slice(&c);
    }
    let total_len = body.len() as u32;
    body[0..4].copy_from_slice(&total_len.to_be_bytes());
    body
}

fn clean_stsd_box(stsd_data: &[u8]) -> Vec<u8> {
    if stsd_data.len() < 16 {
        return stsd_data.to_vec();
    }
    let version_flags = stsd_data[8..12].to_vec();
    let entry_count = read_u32be(stsd_data, 12) as usize;

    let mut entries: Vec<Vec<u8>> = Vec::new();
    let mut off = 16usize;
    for _ in 0..entry_count {
        if off + 8 > stsd_data.len() {
            break;
        }
        let sz = read_u32be(stsd_data, off) as usize;
        if sz < 8 || off + sz > stsd_data.len() {
            break;
        }
        entries.push(clean_stsd_entry(&stsd_data[off..off + sz]));
        off += sz;
    }

    let ec_buf = (entries.len() as u32).to_be_bytes();
    let mut body: Vec<u8> = version_flags;
    body.extend_from_slice(&ec_buf);
    for e in &entries {
        body.extend_from_slice(e);
    }

    let mut out = Vec::with_capacity(8 + body.len());
    let total = (8 + body.len()) as u32;
    out.extend_from_slice(&total.to_be_bytes());
    out.extend_from_slice(b"stsd");
    out.extend_from_slice(&body);
    out
}

pub fn rebuild_box(data: &[u8], transforms: &BoxTransforms) -> Vec<u8> {
    if data.len() < 8 {
        return data.to_vec();
    }
    let box_type = std::str::from_utf8(&data[4..8]).unwrap_or("????").to_string();
    let hdr_size = 8usize;
    let boxes = read_boxes_range(data, hdr_size, data.len());

    let mut new_parts: Vec<Vec<u8>> = Vec::new();
    for (t, off, size, _hdr) in &boxes {
        let box_data = &data[*off..*off + *size];
        if t == "stsd" {
            if let Some(f) = transforms.stsd {
                new_parts.push(f(box_data));
                continue;
            }
        }
        if is_container_type(t) {
            new_parts.push(rebuild_box(box_data, transforms));
        } else {
            new_parts.push(box_data.to_vec());
        }
    }

    let mut body: Vec<u8> = Vec::new();
    for p in new_parts {
        body.extend_from_slice(&p);
    }

    let total = (8 + body.len()) as u32;
    let mut out = Vec::with_capacity(8 + body.len());
    out.extend_from_slice(&total.to_be_bytes());
    out.extend_from_slice(box_type.as_bytes());
    out.extend_from_slice(&body);
    out
}

pub fn clean_moov(moov_data: &[u8]) -> Vec<u8> {
    rebuild_box(moov_data, &BoxTransforms { stsd: Some(clean_stsd_box) })
}

fn make_box(box_type: &str, body: &[u8]) -> Vec<u8> {
    let total = (8 + body.len()) as u32;
    let mut out = Vec::with_capacity(8 + body.len());
    out.extend_from_slice(&total.to_be_bytes());
    out.extend_from_slice(box_type.as_bytes());
    out.extend_from_slice(body);
    out
}

fn make_full_box(box_type: &str, version: u8, flags: u32, body: &[u8]) -> Vec<u8> {
    let mut vf = [0u8; 4];
    vf[0] = version;
    vf[1] = ((flags >> 16) & 0xFF) as u8;
    vf[2] = ((flags >> 8) & 0xFF) as u8;
    vf[3] = (flags & 0xFF) as u8;
    let mut combined = vf.to_vec();
    combined.extend_from_slice(body);
    make_box(box_type, &combined)
}

pub fn build_stts(entries: &[(u32, u32)]) -> Vec<u8> {
    let mut body = Vec::with_capacity(4 + entries.len() * 8);
    body.extend_from_slice(&(entries.len() as u32).to_be_bytes());
    for (count, duration) in entries {
        body.extend_from_slice(&count.to_be_bytes());
        body.extend_from_slice(&duration.to_be_bytes());
    }
    make_full_box("stts", 0, 0, &body)
}

pub fn build_stsc(entries: &[(u32, u32, u32)]) -> Vec<u8> {
    let mut body = Vec::with_capacity(4 + entries.len() * 12);
    body.extend_from_slice(&(entries.len() as u32).to_be_bytes());
    for (first_chunk, spc, sdi) in entries {
        body.extend_from_slice(&first_chunk.to_be_bytes());
        body.extend_from_slice(&spc.to_be_bytes());
        body.extend_from_slice(&sdi.to_be_bytes());
    }
    make_full_box("stsc", 0, 0, &body)
}

pub fn build_stsz(sizes: &[u32]) -> Vec<u8> {
    let mut body = Vec::with_capacity(8 + sizes.len() * 4);
    body.extend_from_slice(&0u32.to_be_bytes()); // sample_size = 0 (variable)
    body.extend_from_slice(&(sizes.len() as u32).to_be_bytes());
    for s in sizes {
        body.extend_from_slice(&s.to_be_bytes());
    }
    make_full_box("stsz", 0, 0, &body)
}

pub fn build_stco(offsets: &[u32]) -> Vec<u8> {
    let mut body = Vec::with_capacity(4 + offsets.len() * 4);
    body.extend_from_slice(&(offsets.len() as u32).to_be_bytes());
    for o in offsets {
        body.extend_from_slice(&o.to_be_bytes());
    }
    make_full_box("stco", 0, 0, &body)
}

fn extract_clean_stsd_for_audio(moov_data: &[u8], audio_track_id: u32) -> Option<Vec<u8>> {
    let boxes = read_boxes_range(moov_data, 8, moov_data.len());
    for (t, off, size, _) in &boxes {
        if t != "trak" {
            continue;
        }
        let trak_data = &moov_data[*off..*off + *size];
        let tkhd = find_child_box(trak_data, "tkhd")?;
        let tkhd_data = &trak_data[tkhd.1..tkhd.1 + tkhd.2];
        let version = tkhd_data[8];
        let tid_off = if version == 0 { 20usize } else { 28usize };
        if tid_off + 4 > tkhd_data.len() {
            continue;
        }
        if read_u32be(tkhd_data, tid_off) != audio_track_id {
            continue;
        }

        let mdia = find_child_box(trak_data, "mdia")?;
        let mdia_data = &trak_data[mdia.1..mdia.1 + mdia.2];
        let minf = find_child_box(mdia_data, "minf")?;
        let minf_data = &mdia_data[minf.1..minf.1 + minf.2];
        let stbl = find_child_box_skip(minf_data, "stbl", 8)?;
        let stbl_data = &minf_data[stbl.1..stbl.1 + stbl.2];
        let stsd = find_child_box_skip(stbl_data, "stsd", 8)?;
        let stsd_data = &stbl_data[stsd.1..stsd.1 + stsd.2];
        if stsd_data.len() < 16 {
            continue;
        }

        let first_entry_off = 16usize;
        if first_entry_off + 8 > stsd_data.len() {
            continue;
        }
        let entry_sz = read_u32be(stsd_data, first_entry_off) as usize;
        if entry_sz < 8 || first_entry_off + entry_sz > stsd_data.len() {
            continue;
        }
        let first_entry = &stsd_data[first_entry_off..first_entry_off + entry_sz];
        let cleaned_entry = clean_stsd_entry(first_entry);

        let vf = stsd_data[8..12].to_vec();
        let ec = 1u32.to_be_bytes();
        let mut body = vf;
        body.extend_from_slice(&ec);
        body.extend_from_slice(&cleaned_entry);
        return Some(make_box("stsd", &body));
    }
    None
}

fn patch_box_duration(box_data: &[u8], total_duration: u32) -> Vec<u8> {
    let mut out = box_data.to_vec();
    let version = out[8];
    if version == 0 {
        out[24..28].copy_from_slice(&total_duration.to_be_bytes());
    } else {
        out[32..36].copy_from_slice(&0u32.to_be_bytes());
        out[36..40].copy_from_slice(&total_duration.to_be_bytes());
    }
    out
}

fn patch_tkhd_duration(box_data: &[u8], total_duration: u32) -> Vec<u8> {
    let mut out = box_data.to_vec();
    out[9] = 0x00;
    out[10] = 0x00;
    out[11] = 0x07;
    let version = out[8];
    if version == 0 {
        out[28..32].copy_from_slice(&total_duration.to_be_bytes());
    } else {
        out[36..40].copy_from_slice(&0u32.to_be_bytes());
        out[40..44].copy_from_slice(&total_duration.to_be_bytes());
    }
    out
}

fn rebuild_minf_stbl(minf_data: &[u8], new_stbl: &[u8]) -> Vec<u8> {
    let boxes = read_boxes_range(minf_data, 8, minf_data.len());
    let mut parts: Vec<Vec<u8>> = Vec::new();
    for (t, off, size, _) in &boxes {
        if t == "stbl" {
            parts.push(new_stbl.to_vec());
        } else {
            parts.push(minf_data[*off..*off + *size].to_vec());
        }
    }
    let mut body = Vec::new();
    for p in parts {
        body.extend_from_slice(&p);
    }
    make_box("minf", &body)
}

fn rebuild_mdia_stbl(mdia_data: &[u8], new_stbl: &[u8], total_duration: u32) -> Vec<u8> {
    let boxes = read_boxes_range(mdia_data, 8, mdia_data.len());
    let mut parts: Vec<Vec<u8>> = Vec::new();
    for (t, off, size, _) in &boxes {
        let box_data = &mdia_data[*off..*off + *size];
        match t.as_str() {
            "mdhd" => parts.push(patch_box_duration(box_data, total_duration)),
            "minf" => parts.push(rebuild_minf_stbl(box_data, new_stbl)),
            _ => parts.push(box_data.to_vec()),
        }
    }
    let mut body = Vec::new();
    for p in parts {
        body.extend_from_slice(&p);
    }
    make_box("mdia", &body)
}

fn rebuild_trak_stbl(trak_data: &[u8], new_stbl: &[u8], total_duration: u32) -> Vec<u8> {
    let boxes = read_boxes_range(trak_data, 8, trak_data.len());
    let mut parts: Vec<Vec<u8>> = Vec::new();
    for (t, off, size, _) in &boxes {
        let box_data = &trak_data[*off..*off + *size];
        match t.as_str() {
            "tkhd" => parts.push(patch_tkhd_duration(box_data, total_duration)),
            "mdia" => parts.push(rebuild_mdia_stbl(box_data, new_stbl, total_duration)),
            _ => parts.push(box_data.to_vec()),
        }
    }
    let mut body = Vec::new();
    for p in parts {
        body.extend_from_slice(&p);
    }
    make_box("trak", &body)
}

fn rebuild_moov_for_non_frag(
    moov_data: &[u8],
    audio_track_id: u32,
    new_stbl: &[u8],
    total_duration: u32,
) -> Vec<u8> {
    let children = read_boxes_range(moov_data, 8, moov_data.len());
    let mut parts: Vec<Vec<u8>> = Vec::new();
    for (t, off, size, _) in &children {
        let box_data = &moov_data[*off..*off + *size];
        match t.as_str() {
            "mvex" => {} // drop fragmentation metadata
            "mvhd" => parts.push(patch_box_duration(box_data, total_duration)),
            "trak" => {
                let mut is_audio = false;
                if let Some(tkhd) = find_child_box(box_data, "tkhd") {
                    let tkhd_data = &box_data[tkhd.1..tkhd.1 + tkhd.2];
                    let ver = tkhd_data[8];
                    let tid_off = if ver == 0 { 20usize } else { 28usize };
                    if tid_off + 4 <= tkhd_data.len()
                        && read_u32be(tkhd_data, tid_off) == audio_track_id
                    {
                        is_audio = true;
                    }
                }
                if is_audio {
                    parts.push(rebuild_trak_stbl(box_data, new_stbl, total_duration));
                } else {
                    parts.push(box_data.to_vec());
                }
            }
            _ => parts.push(box_data.to_vec()),
        }
    }
    let mut body = Vec::new();
    for p in parts {
        body.extend_from_slice(&p);
    }
    make_box("moov", &body)
}

pub fn build_non_fragmented_m4a(fragments: &[Fragment], moov: &[u8]) -> Vec<u8> {
    let samples: Vec<&Sample> = fragments.iter().flat_map(|f| f.samples.iter()).collect();

    let audio_track_id = get_audio_track_id(moov);
    let total_duration: u32 = samples.iter().map(|s| s.duration).sum();

    let clean_stsd = extract_clean_stsd_for_audio(moov, audio_track_id)
        .unwrap_or_else(|| Vec::new());

    let mut stts_runs: Vec<(u32, u32)> = Vec::new();
    for s in &samples {
        if let Some(last) = stts_runs.last_mut() {
            if last.1 == s.duration {
                last.0 += 1;
                continue;
            }
        }
        stts_runs.push((1, s.duration));
    }
    let stts = build_stts(&stts_runs);
    let stsc = build_stsc(&[(1, samples.len() as u32, 1)]);
    let stsz_sizes: Vec<u32> = samples.iter().map(|s| s.data.len() as u32).collect();
    let stsz = build_stsz(&stsz_sizes);
    let stco_placeholder = build_stco(&[0u32]);

    let new_stbl = make_box("stbl", &{
        let mut b = Vec::new();
        b.extend_from_slice(&clean_stsd);
        b.extend_from_slice(&stts);
        b.extend_from_slice(&stsc);
        b.extend_from_slice(&stsz);
        b.extend_from_slice(&stco_placeholder);
        b
    });

    let default_ftyp = make_box("ftyp", b"M4A \x00\x00\x00\x00M4A mp42isom\x00\x00\x00\x00");
    let new_moov = rebuild_moov_for_non_frag(moov, audio_track_id, &new_stbl, total_duration);

    let mdat_data_offset = default_ftyp.len() + new_moov.len() + 8;

    let mut patched_moov = new_moov;
    if let Some(stco_pos) = find_bytes(&patched_moov, b"stco") {
        let box_start = stco_pos - 4;
        if box_start + 20 <= patched_moov.len()
            && read_u32be(&patched_moov, box_start + 12) == 1
        {
            patched_moov[box_start + 16..box_start + 20]
                .copy_from_slice(&(mdat_data_offset as u32).to_be_bytes());
        }
    }

    let dec_data: Vec<u8> = samples.iter().flat_map(|s| s.data.iter().copied()).collect();
    let mdat_size = (8 + dec_data.len()) as u32;
    let mut mdat_hdr = Vec::with_capacity(8);
    mdat_hdr.extend_from_slice(&mdat_size.to_be_bytes());
    mdat_hdr.extend_from_slice(b"mdat");

    let mut out = Vec::new();
    out.extend_from_slice(&default_ftyp);
    out.extend_from_slice(&patched_moov);
    out.extend_from_slice(&mdat_hdr);
    out.extend_from_slice(&dec_data);
    out
}

pub fn get_audio_track_id(moov_data: &[u8]) -> u32 {
    let boxes = read_boxes_range(moov_data, 8, moov_data.len());
    for (t, off, size, _) in &boxes {
        if t != "trak" {
            continue;
        }
        let trak_data = &moov_data[*off..*off + *size];

        if let Some(hdlr_idx) = find_bytes(trak_data, b"hdlr") {
            let handler_off = hdlr_idx + 4 + 4 + 4;
            if handler_off + 4 <= trak_data.len()
                && &trak_data[handler_off..handler_off + 4] == b"soun"
            {
                if let Some(tkhd) = find_child_box(trak_data, "tkhd") {
                    let tkhd_data = &trak_data[tkhd.1..tkhd.1 + tkhd.2];
                    let version = tkhd_data[8];
                    let tid_off = if version == 0 {
                        8 + 4 + 4 + 4
                    } else {
                        8 + 4 + 8 + 8
                    };
                    if tid_off + 4 <= tkhd_data.len() {
                        return read_u32be(tkhd_data, tid_off);
                    }
                }
            }
        }
    }
    1
}

pub fn get_trex_defaults(moov_data: &[u8], track_id: u32) -> TrexDefaults {
    let mut defaults = TrexDefaults {
        default_sample_duration: 1024,
        default_sample_size: 0,
        default_sample_desc_index: 1,
        default_sample_flags: 0,
    };

    let mvex = match find_child_box(moov_data, "mvex") {
        Some(b) => b,
        None => return defaults,
    };
    let mvex_data = &moov_data[mvex.1..mvex.1 + mvex.2];
    let boxes = read_boxes_range(mvex_data, 8, mvex_data.len());
    for (t, off, size, _) in &boxes {
        if t != "trex" || *size < 32 {
            continue;
        }
        let box_data = &mvex_data[*off..*off + *size];
        let tid = read_u32be(box_data, 12);
        if track_id == 0 || tid == track_id {
            defaults.default_sample_desc_index = read_u32be(box_data, 16);
            defaults.default_sample_duration = read_u32be(box_data, 20);
            defaults.default_sample_size = read_u32be(box_data, 24);
            defaults.default_sample_flags = read_u32be(box_data, 28);
            break;
        }
    }
    defaults
}

pub fn extract_enc_info_per_desc(moov_data: &[u8], _track_id: u32) -> Vec<EncInfo> {
    let mut result = Vec::new();
    let boxes = read_boxes_range(moov_data, 8, moov_data.len());
    for (t, off, size, _) in &boxes {
        if t != "trak" {
            continue;
        }
        let trak_data = &moov_data[*off..*off + *size];

        let hdlr_idx = match find_bytes(trak_data, b"hdlr") {
            Some(i) => i,
            None => continue,
        };
        let handler_off = hdlr_idx + 4 + 4 + 4;
        if handler_off + 4 > trak_data.len() {
            continue;
        }
        if &trak_data[handler_off..handler_off + 4] != b"soun" {
            continue;
        }

        let mdia = match find_child_box(trak_data, "mdia") {
            Some(b) => b,
            None => continue,
        };
        let mdia_data = &trak_data[mdia.1..mdia.1 + mdia.2];
        let minf = match find_child_box(mdia_data, "minf") {
            Some(b) => b,
            None => continue,
        };
        let minf_data = &mdia_data[minf.1..minf.1 + minf.2];
        let stbl = match find_child_box(minf_data, "stbl") {
            Some(b) => b,
            None => continue,
        };
        let stbl_data = &minf_data[stbl.1..stbl.1 + stbl.2];
        let stsd = match find_child_box_skip(stbl_data, "stsd", 8) {
            Some(b) => b,
            None => continue,
        };
        let stsd_data = &stbl_data[stsd.1..stsd.1 + stsd.2];
        if stsd_data.len() < 16 {
            continue;
        }

        let mut off2 = 16usize;
        while off2 + 8 <= stsd_data.len() {
            let sz = read_u32be(stsd_data, off2) as usize;
            if sz < 8 || off2 + sz > stsd_data.len() {
                break;
            }
            let entry_data = &stsd_data[off2..off2 + sz];

            let sinf = find_child_box_skip(entry_data, "sinf", 36);
            if let Some(sinf_b) = sinf {
                let sinf_data = &entry_data[sinf_b.1..sinf_b.1 + sinf_b.2];
                let mut scheme_type = "cbcs".to_string();
                let mut per_sample_iv_size = 0u8;
                let mut constant_iv = None;
                let mut key_id = Vec::new();
                let mut iv_size = 0u8;

                if let Some(schm) = find_child_box(sinf_data, "schm") {
                    let schm_data = &sinf_data[schm.1..schm.1 + schm.2];
                    if schm_data.len() >= 20 {
                        let raw = std::str::from_utf8(&schm_data[12..16]).unwrap_or("");
                        scheme_type = raw.trim_matches('\x00').trim().to_string();
                    }
                }

                if let Some(schi) = find_child_box(sinf_data, "schi") {
                    let schi_data = &sinf_data[schi.1..schi.1 + schi.2];
                    if let Some(tenc) = find_child_box(schi_data, "tenc") {
                        let tenc_data = &schi_data[tenc.1..tenc.1 + tenc.2];
                        if tenc_data.len() >= 32 {
                            per_sample_iv_size = tenc_data[15];
                            iv_size = per_sample_iv_size;
                            if tenc_data.len() >= 30 {
                                key_id = tenc_data[14..30].to_vec();
                            }
                            if per_sample_iv_size == 0 && tenc_data.len() > 32 {
                                let civ_size = tenc_data[32] as usize;
                                if civ_size > 0 && tenc_data.len() >= 33 + civ_size {
                                    constant_iv = Some(tenc_data[33..33 + civ_size].to_vec());
                                }
                            }
                        }
                    }
                }

                result.push(EncInfo {
                    scheme_type,
                    key_id,
                    iv_size,
                    per_sample_iv_size,
                    constant_iv,
                });
            }
            off2 += sz;
        }
        return result;
    }
    result
}

pub fn parse_tfhd(data: &[u8]) -> TfhdInfo {
    let mut info = TfhdInfo::default();
    if data.len() < 8 {
        return info;
    }
    let flags = ((data[1] as u32) << 16) | ((data[2] as u32) << 8) | (data[3] as u32);
    info.track_id = read_u32be(data, 4);
    let mut off = 8usize;

    if flags & 0x01 != 0 && off + 8 <= data.len() {
        let hi = read_u32be(data, off) as u64;
        let lo = read_u32be(data, off + 4) as u64;
        info.base_data_offset = Some(hi * 0x100000000 + lo);
        off += 8;
    }
    if flags & 0x02 != 0 && off + 4 <= data.len() {
        info.desc_index = Some(read_u32be(data, off));
        off += 4;
    }
    if flags & 0x08 != 0 && off + 4 <= data.len() {
        info.default_sample_duration = Some(read_u32be(data, off));
        off += 4;
    }
    if flags & 0x10 != 0 && off + 4 <= data.len() {
        info.default_sample_size = Some(read_u32be(data, off));
    }
    info
}

pub fn parse_trun(data: &[u8], _default_flags: u32) -> Vec<TrunSample> {
    if data.len() < 8 {
        return Vec::new();
    }
    let flags = ((data[1] as u32) << 16) | ((data[2] as u32) << 8) | (data[3] as u32);
    let sample_count = read_u32be(data, 4) as usize;
    let mut off = 8usize;

    if flags & 0x01 != 0 { off += 4; } // data_offset
    if flags & 0x04 != 0 { off += 4; } // first_sample_flags

    let mut entries = Vec::with_capacity(sample_count);
    for _ in 0..sample_count {
        let mut entry = TrunSample {
            duration: None,
            size: None,
            flags: None,
            ct_offset: None,
        };
        if flags & 0x100 != 0 && off + 4 <= data.len() {
            entry.duration = Some(read_u32be(data, off));
            off += 4;
        }
        if flags & 0x200 != 0 && off + 4 <= data.len() {
            entry.size = Some(read_u32be(data, off));
            off += 4;
        }
        if flags & 0x400 != 0 && off + 4 <= data.len() {
            entry.flags = Some(read_u32be(data, off));
            off += 4;
        }
        if flags & 0x800 != 0 && off + 4 <= data.len() {
            entry.ct_offset = Some(read_i32be(data, off));
            off += 4;
        }
        entries.push(entry);
    }
    entries
}

fn parse_trun_with_data_offset(data: &[u8]) -> (Vec<TrunSample>, Option<i32>) {
    if data.len() < 8 {
        return (Vec::new(), None);
    }
    let flags = ((data[1] as u32) << 16) | ((data[2] as u32) << 8) | (data[3] as u32);
    let sample_count = read_u32be(data, 4) as usize;
    let mut off = 8usize;
    let mut data_offset = None;

    if flags & 0x01 != 0 && off + 4 <= data.len() {
        data_offset = Some(read_i32be(data, off));
        off += 4;
    }
    if flags & 0x04 != 0 { off += 4; }

    let mut entries = Vec::with_capacity(sample_count);
    for _ in 0..sample_count {
        let mut entry = TrunSample {
            duration: None,
            size: None,
            flags: None,
            ct_offset: None,
        };
        if flags & 0x100 != 0 && off + 4 <= data.len() {
            entry.duration = Some(read_u32be(data, off));
            off += 4;
        }
        if flags & 0x200 != 0 && off + 4 <= data.len() {
            entry.size = Some(read_u32be(data, off));
            off += 4;
        }
        if flags & 0x400 != 0 { off += 4; }
        if flags & 0x800 != 0 { off += 4; }
        entries.push(entry);
    }
    (entries, data_offset)
}

pub fn parse_senc(data: &[u8]) -> Vec<SencEntry> {
    if data.len() < 8 {
        return Vec::new();
    }
    parse_senc_with_iv_size(data, 8)
}

fn parse_senc_with_iv_size(data: &[u8], per_sample_iv_size: u8) -> Vec<SencEntry> {
    if data.len() < 8 {
        return Vec::new();
    }
    let flags = ((data[1] as u32) << 16) | ((data[2] as u32) << 8) | (data[3] as u32);
    let sample_count = read_u32be(data, 4) as usize;
    let mut off = 8usize;
    let mut entries = Vec::with_capacity(sample_count);

    for _ in 0..sample_count {
        let mut iv = Vec::new();
        if per_sample_iv_size > 0 {
            if off + per_sample_iv_size as usize > data.len() {
                break;
            }
            iv = data[off..off + per_sample_iv_size as usize].to_vec();
            off += per_sample_iv_size as usize;
        }
        let mut subsamples = Vec::new();
        if flags & 0x02 != 0 {
            if off + 2 > data.len() {
                break;
            }
            let ss_count = read_u16be(data, off) as usize;
            off += 2;
            for _ in 0..ss_count {
                if off + 6 > data.len() {
                    break;
                }
                let clear = read_u16be(data, off);
                let enc = read_u32be(data, off + 2);
                subsamples.push((clear, enc));
                off += 6;
            }
        }
        entries.push(SencEntry { iv, subsamples });
    }
    entries
}

pub fn parse_moof_mdat(data: &[u8]) -> Vec<Fragment> {
    let boxes = read_boxes_range(data, 0, data.len());
    let mut fragments = Vec::new();
    let mut pending_moof: Option<(usize, usize)> = None; // (offset, size)

    for (t, off, size, _) in &boxes {
        if t == "moof" {
            pending_moof = Some((*off, *size));
        } else if t == "mdat" {
            if let Some((moof_off, moof_size)) = pending_moof.take() {
                let moof = data[moof_off..moof_off + moof_size].to_vec();
                let mdat = data[*off..*off + *size].to_vec();
                fragments.push(Fragment {
                    moof,
                    mdat,
                    samples: Vec::new(),
                });
            }
        }
    }
    fragments
}

fn extract_samples_from_fragment(
    fragment: &Fragment,
    defaults: &TrexDefaults,
    audio_track_id: u32,
    moof_offset: usize,
    per_sample_iv_size: u8,
) -> Vec<Sample> {
    let moof_data = &fragment.moof;
    let mdat_data = &fragment.mdat;
    let mdat_hdr_size = 8usize; // standard box header
    let mdat_data_content = &mdat_data[mdat_hdr_size.min(mdat_data.len())..];
    let mdat_data_offset = moof_offset + fragment.moof.len() + mdat_hdr_size;

    let mut samples = Vec::new();
    let boxes = read_boxes_range(moof_data, 8, moof_data.len());

    for (t, off, size, _) in &boxes {
        if t != "traf" {
            continue;
        }
        let traf_data = &moof_data[*off..*off + *size];

        let mut tfhd_track_id = 0u32;
        let mut tfhd_desc_index: Option<u32> = None;
        let mut tfhd_base_data_offset: Option<u64> = None;
        let mut tfhd_default_duration = defaults.default_sample_duration;
        let mut tfhd_default_size = defaults.default_sample_size;
        let mut trun_entries: Vec<TrunSample> = Vec::new();
        let mut first_trun_data_offset: Option<i32> = None;
        let mut senc_entries: Vec<SencEntry> = Vec::new();

        let traf_boxes = read_boxes_range(traf_data, 8, traf_data.len());
        for (tb_type, tb_off, tb_size, tb_hdr) in &traf_boxes {
            let tb_data = &traf_data[*tb_off..*tb_off + *tb_size];
            let tb_body = &tb_data[*tb_hdr..];

            match tb_type.as_str() {
                "tfhd" => {
                    let parsed = parse_tfhd(tb_body);
                    tfhd_track_id = parsed.track_id;
                    if parsed.desc_index.map(|d| d != 0).unwrap_or(false) {
                        tfhd_desc_index = parsed.desc_index;
                    }
                    if let Some(bdo) = parsed.base_data_offset {
                        tfhd_base_data_offset = Some(bdo);
                    }
                    if let Some(dd) = parsed.default_sample_duration {
                        tfhd_default_duration = dd;
                    }
                    if let Some(ds) = parsed.default_sample_size {
                        tfhd_default_size = ds;
                    }
                }
                "trun" => {
                    let (entries, do_off) = parse_trun_with_data_offset(tb_body);
                    if first_trun_data_offset.is_none() {
                        first_trun_data_offset = do_off;
                    }
                    trun_entries.extend(entries);
                }
                "senc" => {
                    senc_entries = parse_senc_with_iv_size(tb_body, per_sample_iv_size);
                }
                _ => {}
            }
        }

        if tfhd_track_id != audio_track_id {
            continue;
        }

        let base = tfhd_base_data_offset.unwrap_or(moof_offset as u64);
        let mdat_idx = if let Some(do_off) = first_trun_data_offset {
            let abs = base as i64 + do_off as i64;
            let rel = abs - mdat_data_offset as i64;
            rel.max(0) as usize
        } else {
            0
        };
        let mut read_off = mdat_idx;

        let raw_desc_index = tfhd_desc_index
            .unwrap_or(defaults.default_sample_desc_index);
        let desc_index = if raw_desc_index > 0 {
            (raw_desc_index - 1) as usize
        } else {
            0
        };

        for (i, entry) in trun_entries.iter().enumerate() {
            let sample_size = entry.size.unwrap_or(tfhd_default_size) as usize;
            let sample_duration = entry.duration.unwrap_or(tfhd_default_duration);

            if sample_size > 0 && read_off + sample_size <= mdat_data_content.len() {
                let sample_iv = if i < senc_entries.len() {
                    senc_entries[i].iv.clone()
                } else {
                    Vec::new()
                };
                let sample_subsamples = if i < senc_entries.len() {
                    senc_entries[i].subsamples.clone()
                } else {
                    Vec::new()
                };
                samples.push(Sample {
                    data: mdat_data_content[read_off..read_off + sample_size].to_vec(),
                    iv: sample_iv,
                    subsamples: sample_subsamples,
                    duration: sample_duration,
                    desc_index,
                });
                read_off += sample_size;
            }
        }
    }
    samples
}

type Aes128Cbc = cbc::Decryptor<Aes128>;
type Aes128Ctr = ctr::Ctr128BE<Aes128>;

pub fn decrypt_samples(
    samples: &[Sample],
    enc_info: &EncInfo,
    key: &[u8; 16],
) -> Vec<Vec<u8>> {
    let is_cenc = enc_info.scheme_type == "cenc";
    let mut result = Vec::with_capacity(samples.len());

    for sample in samples {
        if is_cenc {
            let mut iv = [0u8; 16];
            if !sample.iv.is_empty() {
                let copy_len = sample.iv.len().min(16);
                iv[..copy_len].copy_from_slice(&sample.iv[..copy_len]);
            }

            if !sample.subsamples.is_empty() {
                let mut plaintext = Vec::with_capacity(sample.data.len());
                let mut off = 0usize;
                for (clear_bytes, enc_bytes) in &sample.subsamples {
                    let cb = *clear_bytes as usize;
                    let eb = *enc_bytes as usize;
                    if off + cb <= sample.data.len() {
                        plaintext.extend_from_slice(&sample.data[off..off + cb]);
                        off += cb;
                    }
                    if eb > 0 && off + eb <= sample.data.len() {
                        let mut buf = sample.data[off..off + eb].to_vec();
                        if let Ok(mut cipher) = Aes128Ctr::new_from_slices(key, &iv) {
                            cipher.apply_keystream(&mut buf);
                        }
                        plaintext.extend_from_slice(&buf);
                        off += eb;
                    }
                }
                if off < sample.data.len() {
                    plaintext.extend_from_slice(&sample.data[off..]);
                }
                result.push(plaintext);
            } else {
                let mut buf = sample.data.clone();
                if let Ok(mut cipher) = Aes128Ctr::new_from_slices(key, &iv) {
                    cipher.apply_keystream(&mut buf);
                }
                result.push(buf);
            }
        } else {
            let mut iv = [0u8; 16];
            if !sample.iv.is_empty() {
                let copy_len = sample.iv.len().min(16);
                iv[..copy_len].copy_from_slice(&sample.iv[..copy_len]);
            } else if let Some(civ) = &enc_info.constant_iv {
                let copy_len = civ.len().min(16);
                iv[..copy_len].copy_from_slice(&civ[..copy_len]);
            }

            if !sample.subsamples.is_empty() {
                let mut enc_parts: Vec<Vec<u8>> = Vec::new();
                let mut enc_sizes: Vec<usize> = Vec::new();
                let mut off = 0usize;
                for (clear_bytes, enc_bytes) in &sample.subsamples {
                    off += *clear_bytes as usize;
                    let eb = *enc_bytes as usize;
                    if eb > 0 && off + eb <= sample.data.len() {
                        enc_parts.push(sample.data[off..off + eb].to_vec());
                        enc_sizes.push(eb);
                    }
                    off += eb;
                }

                let dec_concat: Vec<u8> = if !enc_parts.is_empty() {
                    let enc_concat: Vec<u8> = enc_parts.concat();
                    let cbc_len = enc_concat.len() & !0xF;
                    if cbc_len > 0 {
                        let mut buf = enc_concat[..cbc_len].to_vec();
                        let cipher = Aes128Cbc::new_from_slices(key, &iv)
                            .expect("valid cbc key/iv");
                        let _ = cipher.decrypt_padded_mut::<NoPadding>(&mut buf);
                        let dec = buf.to_vec();
                        let mut r = dec;
                        r.extend_from_slice(&enc_concat[cbc_len..]);
                        r
                    } else {
                        enc_concat
                    }
                } else {
                    Vec::new()
                };

                let mut plaintext = Vec::with_capacity(sample.data.len());
                let mut dec_off = 0usize;
                let mut off2 = 0usize;
                for (clear_bytes, enc_bytes) in &sample.subsamples {
                    let cb = *clear_bytes as usize;
                    let eb = *enc_bytes as usize;
                    if off2 + cb <= sample.data.len() {
                        plaintext.extend_from_slice(&sample.data[off2..off2 + cb]);
                        off2 += cb;
                    }
                    if eb > 0 {
                        if dec_off + eb <= dec_concat.len() {
                            plaintext.extend_from_slice(&dec_concat[dec_off..dec_off + eb]);
                        }
                        dec_off += eb;
                        off2 += eb;
                    }
                }
                if off2 < sample.data.len() {
                    plaintext.extend_from_slice(&sample.data[off2..]);
                }
                result.push(plaintext);
            } else {
                let sample_len = sample.data.len();
                let cbc_len = sample_len & !0xF;
                if cbc_len == 0 {
                    result.push(sample.data.clone());
                } else {
                    let mut buf = sample.data[..cbc_len].to_vec();
                    let cipher = Aes128Cbc::new_from_slices(key, &iv)
                        .expect("valid cbc key/iv");
                    let _ = cipher.decrypt_padded_mut::<NoPadding>(&mut buf);
                    let dec = buf.to_vec();
                    if cbc_len < sample_len {
                        let mut r = dec;
                        r.extend_from_slice(&sample.data[cbc_len..]);
                        result.push(r);
                    } else {
                        result.push(dec);
                    }
                }
            }
        }
    }
    result
}

fn fix_trun_data_offset(trun_box_data: &[u8], delta: i32) -> Vec<u8> {
    let flags = ((trun_box_data[9] as u32) << 16)
        | ((trun_box_data[10] as u32) << 8)
        | trun_box_data[11] as u32;
    if flags & 0x01 == 0 {
        return trun_box_data.to_vec();
    }
    let mut out = trun_box_data.to_vec();
    let current = read_i32be(&out, 16);
    let new_val = current + delta;
    out[16..20].copy_from_slice(&new_val.to_be_bytes());
    out
}

fn remove_senc_from_traf(traf_data: &[u8]) -> Vec<u8> {
    let children = read_boxes_range(traf_data, 8, traf_data.len());
    let mut senc_removed = 0i32;
    for (t, _, size, _) in &children {
        if t == "senc" {
            senc_removed += *size as i32;
        }
    }
    let mut parts: Vec<Vec<u8>> = Vec::new();
    for (t, off, size, _) in &children {
        let box_data = &traf_data[*off..*off + *size];
        match t.as_str() {
            "senc" => {} // skip
            "trun" if senc_removed > 0 => {
                parts.push(fix_trun_data_offset(box_data, -senc_removed));
            }
            _ => parts.push(box_data.to_vec()),
        }
    }
    let mut body = Vec::new();
    for p in parts {
        body.extend_from_slice(&p);
    }
    make_box("traf", &body)
}

fn remove_senc_from_moof(moof_data: &[u8]) -> Vec<u8> {
    let children = read_boxes_range(moof_data, 8, moof_data.len());
    let mut parts: Vec<Vec<u8>> = Vec::new();
    for (t, off, size, _) in &children {
        let box_data = &moof_data[*off..*off + *size];
        if t == "traf" {
            parts.push(remove_senc_from_traf(box_data));
        } else {
            parts.push(box_data.to_vec());
        }
    }
    let mut body = Vec::new();
    for p in parts {
        body.extend_from_slice(&p);
    }
    make_box("moof", &body)
}

pub fn decrypt_mp4(data: &[u8], key_hex: &str) -> MhResult<Vec<u8>> {
    let state = create_decrypt_state(data)?;
    let mut state = state;
    if state.header.is_empty() {
        let out = decrypt_segment_buf(&mut state, data, key_hex)?;
        return Ok(out);
    }
    Ok(state.header)
}

pub fn create_decrypt_state(init_buf: &[u8]) -> MhResult<DecryptState> {
    let top_boxes = read_boxes_range(init_buf, 0, init_buf.len());
    let mut ftyp_data: Option<Vec<u8>> = None;
    let mut moov_data: Option<Vec<u8>> = None;
    let mut pairs: Vec<(usize, usize, usize, usize)> = Vec::new(); // (moof_off, moof_size, mdat_off, mdat_size)
    let mut pending_moof: Option<(usize, usize)> = None;

    for (t, off, size, _) in &top_boxes {
        match t.as_str() {
            "ftyp" => ftyp_data = Some(init_buf[*off..*off + *size].to_vec()),
            "moov" => moov_data = Some(init_buf[*off..*off + *size].to_vec()),
            "moof" => pending_moof = Some((*off, *size)),
            "mdat" => {
                if let Some((mo, ms)) = pending_moof.take() {
                    pairs.push((mo, ms, *off, *size));
                }
            }
            _ => {}
        }
    }

    let moov = moov_data.ok_or_else(|| MhError::Crypto("no moov box in init segment".to_string()))?;

    let audio_track_id = get_audio_track_id(&moov);
    let trex_defaults = get_trex_defaults(&moov, audio_track_id);
    let enc_infos = extract_enc_info_per_desc(&moov, audio_track_id);

    let per_sample_iv_size = enc_infos.first().map(|e| e.per_sample_iv_size).unwrap_or(0);

    let mut enc_info_map = std::collections::HashMap::new();
    for (i, ei) in enc_infos.iter().enumerate() {
        enc_info_map.insert(i, EncInfoDesc {
            scheme_type: ei.scheme_type.clone(),
            per_sample_iv_size: ei.per_sample_iv_size,
            constant_iv: ei.constant_iv.clone(),
        });
    }
    let enc_info_per_desc = if enc_info_map.is_empty() { None } else { Some(enc_info_map) };

    let cleaned_moov = rebuild_box(&moov, &BoxTransforms { stsd: Some(clean_stsd_box) });

    let mut header_parts: Vec<Vec<u8>> = Vec::new();
    if let Some(ftyp) = ftyp_data {
        header_parts.push(ftyp);
    }
    header_parts.push(cleaned_moov);

    let keys = [None, None];

    let mut state = DecryptState {
        track_id: audio_track_id,
        enc_infos,
        trex_defaults,
        fragments: Vec::new(),
        moov,
        keys,
        per_sample_iv_size,
        enc_info_per_desc,
        pair_count: 0,
        header: Vec::new(),
    };

    for (moof_off, moof_size, mdat_off, mdat_size) in &pairs {
        let moof_bytes = init_buf[*moof_off..*moof_off + *moof_size].to_vec();
        let mdat_bytes = init_buf[*mdat_off..*mdat_off + *mdat_size].to_vec();
        let fragment = Fragment {
            moof: moof_bytes,
            mdat: mdat_bytes,
            samples: Vec::new(),
        };
        let pair_result = process_pair_internal(
            &fragment,
            &state.trex_defaults,
            state.track_id,
            *moof_off,
            state.per_sample_iv_size,
            &state.enc_info_per_desc,
            &state.keys,
        );
        header_parts.push(pair_result);
        state.pair_count += 1;
    }

    state.header = header_parts.concat();
    Ok(state)
}

fn process_pair_internal(
    fragment: &Fragment,
    defaults: &TrexDefaults,
    audio_track_id: u32,
    moof_offset: usize,
    per_sample_iv_size: u8,
    enc_info_per_desc: &Option<std::collections::HashMap<usize, EncInfoDesc>>,
    keys: &[Option<Vec<u8>>; 2],
) -> Vec<u8> {
    let samples = extract_samples_from_fragment(
        fragment,
        defaults,
        audio_track_id,
        moof_offset,
        per_sample_iv_size,
    );

    let mut decrypted_parts: Vec<Vec<u8>> = Vec::new();
    for sample in &samples {
        let key = keys[sample.desc_index.min(1)].as_deref()
            .or_else(|| keys[0].as_deref());
        let Some(key) = key else {
            decrypted_parts.push(sample.data.clone());
            continue;
        };
        if key.len() != 16 {
            decrypted_parts.push(sample.data.clone());
            continue;
        }
        let key16: &[u8; 16] = key.try_into().unwrap();

        let default_enc_info = EncInfo {
            scheme_type: "cbcs".to_string(),
            key_id: Vec::new(),
            iv_size: 0,
            per_sample_iv_size: 0,
            constant_iv: Some(vec![0u8; 16]),
        };
        let enc_info = enc_info_per_desc
            .as_ref()
            .and_then(|m| m.get(&sample.desc_index))
            .map(|d| EncInfo {
                scheme_type: d.scheme_type.clone(),
                key_id: Vec::new(),
                iv_size: 0,
                per_sample_iv_size: d.per_sample_iv_size,
                constant_iv: d.constant_iv.clone(),
            })
            .unwrap_or(default_enc_info);

        let decrypted = decrypt_samples(std::slice::from_ref(sample), &enc_info, key16);
        for d in decrypted {
            decrypted_parts.push(d);
        }
    }

    let decrypted_buf: Vec<u8> = decrypted_parts.concat();
    let cleaned_moof = remove_senc_from_moof(&fragment.moof);

    let mdat_size = (8 + decrypted_buf.len()) as u32;
    let mut mdat_hdr = [0u8; 8];
    mdat_hdr[..4].copy_from_slice(&mdat_size.to_be_bytes());
    mdat_hdr[4..8].copy_from_slice(b"mdat");

    let mut out = Vec::new();
    out.extend_from_slice(&cleaned_moof);
    out.extend_from_slice(&mdat_hdr);
    out.extend_from_slice(&decrypted_buf);
    out
}

pub fn decrypt_segment_buf(state: &mut DecryptState, seg_buf: &[u8], key_hex: &str) -> MhResult<Vec<u8>> {
    let key = hex::decode(key_hex)
        .map_err(|e| MhError::Crypto(format!("invalid key hex: {}", e)))?;
    if key.len() != 16 {
        return Err(MhError::Crypto("key must be 16 bytes".to_string()));
    }

    let default_key = hex::decode(DEFAULT_SONG_DECRYPTION_KEY)
        .expect("valid default key hex");
    state.keys[0] = Some(default_key);
    state.keys[1] = Some(key);

    let boxes = read_boxes_range(seg_buf, 0, seg_buf.len());
    let mut parts: Vec<Vec<u8>> = Vec::new();
    let mut pending_moof: Option<(usize, usize)> = None;

    for (t, off, size, _) in &boxes {
        if t == "moof" {
            pending_moof = Some((*off, *size));
        } else if t == "mdat" {
            if let Some((mo, ms)) = pending_moof.take() {
                let fragment = Fragment {
                    moof: seg_buf[mo..mo + ms].to_vec(),
                    mdat: seg_buf[*off..*off + *size].to_vec(),
                    samples: Vec::new(),
                };
                let pair_out = process_pair_internal(
                    &fragment,
                    &state.trex_defaults,
                    state.track_id,
                    mo,
                    state.per_sample_iv_size,
                    &state.enc_info_per_desc,
                    &state.keys,
                );
                parts.push(pair_out);
                state.pair_count += 1;
            }
        }
    }

    Ok(parts.concat())
}
