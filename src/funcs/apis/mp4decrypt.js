'use strict';

/**
 * Native JS MP4 decryption for Apple Music CMAF segments.
 * Replaces the Python amdecrypt subprocess to eliminate startup overhead.
 *
 * Supports:
 *  - cenc (AES-128-CTR) for legacy AAC streams
 *  - cbcs (AES-128-CBC) for enhanced HLS streams
 *
 * Usage: decryptMp4(encBuf, trackKeyHex, legacy) → Buffer
 */

const crypto = require('crypto');

const DEFAULT_SONG_DECRYPTION_KEY = Buffer.from('32b8ade1769e26b1ffb8986352793fc6', 'hex');

// Container box types that recursively contain child boxes
const CONTAINER_TYPES = new Set([
    'moov', 'trak', 'mdia', 'minf', 'stbl', 'udta', 'mvex',
    'moof', 'traf', 'sinf', 'schi',
]);

// ── Box parsing helpers ──────────────────────────────────────────────────────

function readBoxes(buf, start, end) {
    if (start === undefined) start = 0;
    if (end === undefined) end = buf.length;
    const boxes = [];
    let off = start;
    while (off + 8 <= end) {
        let size = buf.readUInt32BE(off);
        if (size === 0) break;
        let hdrSize = 8;
        if (size === 1) {
            if (off + 16 > end) break;
            const hi = buf.readUInt32BE(off + 8);
            const lo = buf.readUInt32BE(off + 12);
            size = hi * 0x100000000 + lo;
            hdrSize = 16;
        }
        if (size < hdrSize || off + size > end) break;
        const type = buf.slice(off + 4, off + 8).toString('ascii');
        boxes.push({ type, offset: off, size, hdrSize, data: buf.slice(off, off + size) });
        off += size;
    }
    return boxes;
}

function findChildBox(data, type, skipHeader) {
    if (skipHeader === undefined) skipHeader = 8;
    const boxes = readBoxes(data, skipHeader, data.length);
    return boxes.find(b => b.type === type) || null;
}

// ── stsd cleaning: remove sinf, convert enca → original codec ───────────────

function findOriginalFormat(entryData) {
    const sinfMarker = Buffer.from('sinf');
    const sinfIdx = entryData.indexOf(sinfMarker);
    if (sinfIdx < 4) return null;
    const sinfSize = entryData.readUInt32BE(sinfIdx - 4);
    if (sinfSize < 16) return null;
    const sinf = entryData.slice(sinfIdx - 4, sinfIdx - 4 + sinfSize);
    const frmaIdx = sinf.indexOf(Buffer.from('frma'));
    if (frmaIdx < 4) return null;
    const frmaSize = sinf.readUInt32BE(frmaIdx - 4);
    if (frmaSize !== 12) return null;
    return sinf.slice(frmaIdx + 4, frmaIdx + 8);
}

function cleanStsdEntry(entryData) {
    const entryType = entryData.slice(4, 8).toString('ascii');
    const isEncrypted = ['enca', 'encv', 'encs', 'encm'].includes(entryType);
    if (!isEncrypted && entryData.indexOf(Buffer.from('sinf')) < 0) return entryData;

    const originalFormat = findOriginalFormat(entryData) ||
        Buffer.from(isEncrypted ? 'mp4a' : entryType);

    // Audio sample entries: 36-byte fixed header (size+type+reserved+data_ref_index+audio_data)
    const FIXED_HDR = 36;
    if (entryData.length < FIXED_HDR) return entryData;

    // Rebuild entry header with original format, skip sinf in children
    const newHdr = Buffer.from(entryData.slice(0, FIXED_HDR));
    originalFormat.copy(newHdr, 4);

    let off = FIXED_HDR;
    const children = [];
    while (off + 8 <= entryData.length) {
        const sz = entryData.readUInt32BE(off);
        const ct = entryData.slice(off + 4, off + 8).toString('ascii');
        if (sz < 8 || off + sz > entryData.length) break;
        if (ct !== 'sinf') children.push(entryData.slice(off, off + sz));
        off += sz;
    }

    const body = Buffer.concat([newHdr, ...children]);
    const result = Buffer.alloc(body.length);
    body.copy(result);
    result.writeUInt32BE(result.length, 0); // fix size
    return result;
}

function cleanStsdBox(stsdData) {
    if (stsdData.length < 16) return stsdData;
    // stsd FullBox: size(4)+type(4)+version+flags(4)+entry_count(4)+entries
    const versionFlags = stsdData.slice(8, 12);
    const entryCount = stsdData.readUInt32BE(12);

    const entries = [];
    let off = 16;
    for (let i = 0; i < entryCount; i++) {
        if (off + 8 > stsdData.length) break;
        const sz = stsdData.readUInt32BE(off);
        if (sz < 8 || off + sz > stsdData.length) break;
        entries.push(cleanStsdEntry(stsdData.slice(off, off + sz)));
        off += sz;
    }

    const ecBuf = Buffer.alloc(4);
    ecBuf.writeUInt32BE(entries.length);
    const body = Buffer.concat([versionFlags, ecBuf, ...entries]);
    const out = Buffer.alloc(8 + body.length);
    out.writeUInt32BE(out.length, 0);
    out.write('stsd', 4, 'ascii');
    body.copy(out, 8);
    return out;
}

/**
 * Rebuild a container box, applying transforms to specific child box types.
 * Only recurses into known container types.
 */
function rebuildBox(data, transforms) {
    const type = data.slice(4, 8).toString('ascii');
    const hdrSize = 8;
    const boxes = readBoxes(data, hdrSize, data.length);

    const newParts = [];
    for (const box of boxes) {
        if (transforms[box.type]) {
            newParts.push(transforms[box.type](box.data));
        } else if (CONTAINER_TYPES.has(box.type)) {
            newParts.push(rebuildBox(box.data, transforms));
        } else {
            newParts.push(box.data);
        }
    }

    const body = Buffer.concat(newParts);
    const out = Buffer.alloc(8 + body.length);
    out.writeUInt32BE(out.length, 0);
    out.write(type, 4, 'ascii');
    body.copy(out, 8);
    return out;
}

function cleanMoov(moovData) {
    return rebuildBox(moovData, { stsd: cleanStsdBox });
}

// ── Non-fragmented M4A builder ────────────────────────────────────────────────

/** Write a box: 4-byte big-endian size + 4-char type + body */
function makeBox(type, body) {
    const out = Buffer.alloc(8 + body.length);
    out.writeUInt32BE(out.length, 0);
    out.write(type, 4, 'ascii');
    body.copy(out, 8);
    return out;
}

/** Write a FullBox: makeBox with version(1) + flags(3) prepended to body */
function makeFullBox(type, version, flags, body) {
    const vf = Buffer.alloc(4);
    vf[0] = version & 0xff;
    vf[1] = (flags >> 16) & 0xff;
    vf[2] = (flags >> 8) & 0xff;
    vf[3] = flags & 0xff;
    return makeBox(type, Buffer.concat([vf, body]));
}

function buildStts(samples) {
    const runs = [];
    for (const s of samples) {
        if (runs.length > 0 && runs[runs.length - 1][1] === s.duration) {
            runs[runs.length - 1][0]++;
        } else {
            runs.push([1, s.duration]);
        }
    }
    const body = Buffer.alloc(4 + runs.length * 8);
    body.writeUInt32BE(runs.length, 0);
    for (let i = 0; i < runs.length; i++) {
        body.writeUInt32BE(runs[i][0], 4 + i * 8);
        body.writeUInt32BE(runs[i][1], 8 + i * 8);
    }
    return makeFullBox('stts', 0, 0, body);
}

function buildStsc(totalSamples) {
    const body = Buffer.alloc(4 + 12);
    body.writeUInt32BE(1, 0);           // entry_count
    body.writeUInt32BE(1, 4);           // first_chunk
    body.writeUInt32BE(totalSamples, 8); // samples_per_chunk
    body.writeUInt32BE(1, 12);          // sample_description_index (1-based = first entry)
    return makeFullBox('stsc', 0, 0, body);
}

function buildStsz(samples) {
    const body = Buffer.alloc(8 + samples.length * 4);
    body.writeUInt32BE(0, 0);              // sample_size = 0 (variable sizes)
    body.writeUInt32BE(samples.length, 4);
    for (let i = 0; i < samples.length; i++) {
        body.writeUInt32BE(samples[i].data.length, 8 + i * 4);
    }
    return makeFullBox('stsz', 0, 0, body);
}

function buildStco(chunkOffset) {
    const body = Buffer.alloc(8);
    body.writeUInt32BE(1, 0);            // entry_count
    body.writeUInt32BE(chunkOffset, 4);  // chunk_offset
    return makeFullBox('stco', 0, 0, body);
}

/**
 * Extract the cleaned stsd box for the audio track from moov.
 * Converts enca→mp4a and removes sinf boxes. Returns only the first (desc 0) entry.
 */
function extractCleanStsdForAudio(moovData, audioTrackId) {
    const boxes = readBoxes(moovData, 8, moovData.length);
    for (const box of boxes) {
        if (box.type !== 'trak') continue;
        const tkhd = findChildBox(box.data, 'tkhd');
        if (!tkhd) continue;
        const version = tkhd.data[8];
        const tidOff = version === 0 ? 20 : 28;
        if (tidOff + 4 > tkhd.data.length) continue;
        if (tkhd.data.readUInt32BE(tidOff) !== audioTrackId) continue;

        const mdia = findChildBox(box.data, 'mdia');
        if (!mdia) continue;
        const minf = findChildBox(mdia.data, 'minf');
        if (!minf) continue;
        const stbl = findChildBox(minf.data, 'stbl', 8);
        if (!stbl) continue;
        const stsd = findChildBox(stbl.data, 'stsd', 8);
        if (!stsd || stsd.data.length < 16) continue;

        // Parse first entry only (desc 0 — the one all samples reference)
        const entryCountOff = 12; // stsd.data: size(4)+type(4)+ver/flags(4)+entry_count(4)
        const firstEntryOff = 16;
        if (firstEntryOff + 8 > stsd.data.length) continue;
        const entrySz = stsd.data.readUInt32BE(firstEntryOff);
        if (entrySz < 8 || firstEntryOff + entrySz > stsd.data.length) continue;

        const firstEntryData = stsd.data.slice(firstEntryOff, firstEntryOff + entrySz);
        const cleanedEntry = cleanStsdEntry(firstEntryData);

        // Rebuild stsd with just the one cleaned entry
        const vf = stsd.data.slice(8, 12);    // version+flags
        const ec = Buffer.alloc(4); ec.writeUInt32BE(1, 0); // entry_count = 1
        return makeBox('stsd', Buffer.concat([vf, ec, cleanedEntry]));
    }
    return null;
}

/**
 * Patch a FullBox's duration field in-place (copy first).
 * Works for mvhd, mdhd (v0: offset 24, v1: offset 32).
 */
function patchBoxDuration(boxData, totalDuration) {
    const out = Buffer.from(boxData);
    const version = out[8];
    if (version === 0) {
        out.writeUInt32BE(totalDuration >>> 0, 24);
    } else {
        out.writeUInt32BE(0, 32);
        out.writeUInt32BE(totalDuration >>> 0, 36);
    }
    return out;
}

/**
 * Patch tkhd duration + set flags = 7 (enabled|in_movie|in_preview).
 * v0: duration at offset 28; v1: at offset 36.
 */
function patchTkhdDuration(boxData, totalDuration) {
    const out = Buffer.from(boxData);
    // flags = 0x000007 (3 bytes at [9..11])
    out[9] = 0x00; out[10] = 0x00; out[11] = 0x07;
    const version = out[8];
    if (version === 0) {
        out.writeUInt32BE(totalDuration >>> 0, 28);
    } else {
        out.writeUInt32BE(0, 36);
        out.writeUInt32BE(totalDuration >>> 0, 40);
    }
    return out;
}

/**
 * Rebuild the moov box for non-fragmented M4A:
 *  - Remove mvex (fragmentation extension)
 *  - Replace stbl in audio trak with the provided newStbl
 *  - Patch mvhd/tkhd/mdhd duration fields (zero in fragmented source)
 *  - Keep all other boxes unchanged
 */
function rebuildMoovForNonFrag(moovData, audioTrackId, newStbl, totalDuration) {
    const children = readBoxes(moovData, 8, moovData.length);
    const parts = [];
    for (const box of children) {
        if (box.type === 'mvex') continue; // drop fragmentation metadata

        if (box.type === 'mvhd') {
            parts.push(patchBoxDuration(box.data, totalDuration));
            continue;
        }

        if (box.type === 'trak') {
            const tkhd = findChildBox(box.data, 'tkhd');
            if (tkhd) {
                const ver = tkhd.data[8];
                const tidOff = ver === 0 ? 20 : 28;
                if (tidOff + 4 <= tkhd.data.length &&
                    tkhd.data.readUInt32BE(tidOff) === audioTrackId) {
                    parts.push(rebuildTrakStbl(box.data, newStbl, totalDuration));
                    continue;
                }
            }
        }
        parts.push(box.data);
    }
    return makeBox('moov', Buffer.concat(parts));
}

function rebuildTrakStbl(trakData, newStbl, totalDuration) {
    const children = readBoxes(trakData, 8, trakData.length);
    const parts = [];
    for (const box of children) {
        if (box.type === 'tkhd') {
            parts.push(patchTkhdDuration(box.data, totalDuration));
        } else if (box.type === 'mdia') {
            parts.push(rebuildMdiaStbl(box.data, newStbl, totalDuration));
        } else {
            parts.push(box.data);
        }
    }
    return makeBox('trak', Buffer.concat(parts));
}

function rebuildMdiaStbl(mdiaData, newStbl, totalDuration) {
    const children = readBoxes(mdiaData, 8, mdiaData.length);
    const parts = [];
    for (const box of children) {
        if (box.type === 'mdhd') {
            parts.push(patchBoxDuration(box.data, totalDuration));
        } else if (box.type === 'minf') {
            parts.push(rebuildMinfStbl(box.data, newStbl));
        } else {
            parts.push(box.data);
        }
    }
    return makeBox('mdia', Buffer.concat(parts));
}

function rebuildMinfStbl(minfData, newStbl) {
    const children = readBoxes(minfData, 8, minfData.length);
    const parts = [];
    for (const box of children) {
        parts.push(box.type === 'stbl' ? newStbl : box.data);
    }
    return makeBox('minf', Buffer.concat(parts));
}

/**
 * Assemble a clean non-fragmented M4A from decrypted samples.
 * Mirrors Python's write_decrypted_m4a exactly:
 *   ftyp + moov (clean stsd + proper stts/stsc/stsz/stco, no mvex) + mdat
 *
 * @param {Buffer|null} ftypData  - original ftyp box (or null for default)
 * @param {Buffer}      moovData  - original moov box (encrypted, will be cleaned)
 * @param {Array}       samples   - [{data: Buffer, duration: number}, ...]
 * @returns {Buffer}
 */
function buildNonFragmentedM4a(ftypData, moovData, samples) {
    const audioTrackId = getAudioTrackId(moovData);
    const totalDuration = samples.reduce((acc, s) => acc + (s.duration || 0), 0);

    // Build clean stsd (enca→mp4a, sinf removed, first entry only)
    const cleanStsd = extractCleanStsdForAudio(moovData, audioTrackId);
    if (!cleanStsd) throw new Error('mp4decrypt: could not extract clean stsd for audio track');

    // Build sample tables
    const stts = buildStts(samples);
    const stsc = buildStsc(samples.length);
    const stsz = buildStsz(samples);
    const stcoPlaceholder = buildStco(0); // offset fixed after moov size is known

    const newStbl = makeBox('stbl', Buffer.concat([cleanStsd, stts, stsc, stsz, stcoPlaceholder]));
    // Patch mvhd/tkhd/mdhd duration (all zero in fragmented source — Chromium requires correct values)
    const newMoov = rebuildMoovForNonFrag(moovData, audioTrackId, newStbl, totalDuration);

    // ftyp: keep original or write a default M4A ftyp
    const ftyp = ftypData || makeBox('ftyp', Buffer.concat([
        Buffer.from('M4A \x00\x00\x00\x00', 'ascii'),
        Buffer.from('M4A mp42isom\x00\x00\x00\x00', 'ascii'),
    ]));

    // mdat offset = ftyp.length + moov.length + 8 (mdat header)
    const mdatDataOffset = ftyp.length + newMoov.length + 8;

    // Patch the stco chunk_offset in newMoov in-place
    // stco FullBox: size(4)+type(4)+ver/flags(4)+entry_count(4)+chunk_offset(4) = last field at +16
    const stcoMark = Buffer.from('stco');
    let stcoPos = newMoov.indexOf(stcoMark, 8);
    while (stcoPos >= 4) {
        // Confirm this is a proper stco box (entry_count == 1 at +12)
        const boxStart = stcoPos - 4;
        if (boxStart + 20 <= newMoov.length &&
            newMoov.readUInt32BE(boxStart + 12) === 1) {
            newMoov.writeUInt32BE(mdatDataOffset, boxStart + 16);
            break;
        }
        stcoPos = newMoov.indexOf(stcoMark, stcoPos + 1);
    }

    // Build mdat
    const decData = Buffer.concat(samples.map(s => s.data));
    const mdatHdr = Buffer.alloc(8);
    mdatHdr.writeUInt32BE(8 + decData.length, 0);
    mdatHdr.write('mdat', 4, 'ascii');

    return Buffer.concat([ftyp, newMoov, mdatHdr, decData]);
}

// ── Encryption info extraction ───────────────────────────────────────────────

function getAudioTrackId(moovData) {
    const boxes = readBoxes(moovData, 8, moovData.length);
    for (const box of boxes) {
        if (box.type !== 'trak') continue;
        const hdlrIdx = box.data.indexOf(Buffer.from('hdlr'));
        if (hdlrIdx < 4) continue;
        const handlerOff = hdlrIdx + 4 + 4 + 4; // after 'hdlr'+ver+flags+pre_defined
        if (handlerOff + 4 > box.data.length) continue;
        if (box.data.slice(handlerOff, handlerOff + 4).toString('ascii') !== 'soun') continue;
        const tkhd = findChildBox(box.data, 'tkhd');
        if (!tkhd) continue;
        const version = tkhd.data[8];
        // tkhd FullBox: size(4)+type(4)+version(1)+flags(3) then for v0: creation(4)+modification(4)+track_id(4)
        const tidOff = version === 0 ? 8 + 4 + 4 + 4 : 8 + 4 + 8 + 8;
        if (tidOff + 4 <= tkhd.data.length) return tkhd.data.readUInt32BE(tidOff);
    }
    return 1;
}

function getTrexDefaults(moovData, targetTrackId) {
    const defaults = { defaultSampleDuration: 1024, defaultSampleSize: 0, defaultSampleDescIndex: 1 };
    const mvex = findChildBox(moovData, 'mvex');
    if (!mvex) return defaults;
    const boxes = readBoxes(mvex.data, 8, mvex.data.length);
    for (const box of boxes) {
        if (box.type !== 'trex' || box.size < 32) continue;
        // trex FullBox: size(4)+type(4)+version+flags(4)+track_id(4)+desc_idx(4)+dur(4)+size(4)+flags(4)
        const trackId = box.data.readUInt32BE(12);
        if (targetTrackId === 0 || trackId === targetTrackId) {
            defaults.defaultSampleDescIndex = box.data.readUInt32BE(16);
            defaults.defaultSampleDuration = box.data.readUInt32BE(20);
            defaults.defaultSampleSize = box.data.readUInt32BE(24);
            break;
        }
    }
    return defaults;
}

/** Extract per-desc encryption info (schemeType, perSampleIvSize, constantIv) from moov stsd entries. */
function extractEncInfoPerDesc(moovData) {
    const boxes = readBoxes(moovData, 8, moovData.length);
    for (const box of boxes) {
        if (box.type !== 'trak') continue;
        const hdlrIdx = box.data.indexOf(Buffer.from('hdlr'));
        if (hdlrIdx < 4) continue;
        const handlerOff = hdlrIdx + 4 + 4 + 4;
        if (handlerOff + 4 > box.data.length) continue;
        if (box.data.slice(handlerOff, handlerOff + 4).toString('ascii') !== 'soun') continue;

        const mdia = findChildBox(box.data, 'mdia');
        if (!mdia) continue;
        const minf = findChildBox(mdia.data, 'minf');
        if (!minf) continue;
        const stbl = findChildBox(minf.data, 'stbl');
        if (!stbl) continue;
        const stsd = findChildBox(stbl.data, 'stsd', 8);
        if (!stsd || stsd.data.length < 16) continue;

        const result = {};
        let off = 16; // past FullBox header + entry_count
        let descIdx = 0;
        while (off + 8 <= stsd.data.length) {
            const sz = stsd.data.readUInt32BE(off);
            if (sz < 8 || off + sz > stsd.data.length) break;
            const entryData = stsd.data.slice(off, off + sz);
            const sinf = findChildBox(entryData, 'sinf', 36);
            if (sinf) {
                const info = { schemeType: 'cbcs', perSampleIvSize: 0, constantIv: null };
                const schm = findChildBox(sinf.data, 'schm');
                if (schm && schm.data.length >= 20) {
                    info.schemeType = schm.data.slice(12, 16).toString('ascii').replace(/\x00/g, '').trim();
                }
                const schi = findChildBox(sinf.data, 'schi');
                if (schi) {
                    const tenc = findChildBox(schi.data, 'tenc');
                    // tenc FullBox: size(4)+type(4)+version(1)+flags(3)+reserved(2)+
                    //   per_sample_iv_size(1)+KID(16)[ +constant_iv_size(1)+constant_iv ]
                    if (tenc && tenc.data.length >= 32) {
                        info.perSampleIvSize = tenc.data[15];
                        if (info.perSampleIvSize === 0 && tenc.data.length > 32) {
                            const ivSize = tenc.data[32];
                            if (ivSize > 0 && tenc.data.length >= 33 + ivSize) {
                                info.constantIv = tenc.data.slice(33, 33 + ivSize);
                            }
                        }
                    }
                }
                result[descIdx] = info;
            }
            off += sz;
            descIdx++;
        }
        return Object.keys(result).length > 0 ? result : null;
    }
    return null;
}

// ── moof/mdat parsing ────────────────────────────────────────────────────────

function parseTfhd(data) {
    if (data.length < 8) return null;
    // FullBox: version(1)+flags(3)+track_id(4)
    const flags = (data[1] << 16) | (data[2] << 8) | data[3];
    const trackId = data.readUInt32BE(4);
    let off = 8;
    let baseDataOffset = null;
    let descIndex = 0;
    let defaultDuration = null;
    let defaultSize = null;

    if ((flags & 0x01) && off + 8 <= data.length) {
        const hi = data.readUInt32BE(off);
        const lo = data.readUInt32BE(off + 4);
        baseDataOffset = hi * 0x100000000 + lo;
        off += 8;
    }
    if ((flags & 0x02) && off + 4 <= data.length) { descIndex = data.readUInt32BE(off); off += 4; }
    if ((flags & 0x08) && off + 4 <= data.length) { defaultDuration = data.readUInt32BE(off); off += 4; }
    if ((flags & 0x10) && off + 4 <= data.length) { defaultSize = data.readUInt32BE(off); }
    return { trackId, descIndex, baseDataOffset, defaultDuration, defaultSize };
}

function parseTrun(data) {
    if (data.length < 8) return { entries: [], dataOffset: null };
    const flags = (data[1] << 16) | (data[2] << 8) | data[3];
    const sampleCount = data.readUInt32BE(4);
    let off = 8;
    let dataOffset = null;

    if (flags & 0x01) { dataOffset = data.readInt32BE(off); off += 4; }
    if (flags & 0x04) { off += 4; }

    const entries = [];
    for (let i = 0; i < sampleCount; i++) {
        const entry = {};
        if ((flags & 0x100) && off + 4 <= data.length) { entry.duration = data.readUInt32BE(off); off += 4; }
        if ((flags & 0x200) && off + 4 <= data.length) { entry.size = data.readUInt32BE(off); off += 4; }
        if (flags & 0x400) { off += 4; }
        if (flags & 0x800) { off += 4; }
        entries.push(entry);
    }
    return { entries, dataOffset };
}

function parseSenc(data, perSampleIvSize) {
    if (data.length < 8) return [];
    const flags = (data[1] << 16) | (data[2] << 8) | data[3];
    const sampleCount = data.readUInt32BE(4);
    let off = 8;
    const entries = [];

    for (let i = 0; i < sampleCount; i++) {
        let iv = Buffer.alloc(0);
        if (perSampleIvSize > 0) {
            if (off + perSampleIvSize > data.length) break;
            iv = data.slice(off, off + perSampleIvSize);
            off += perSampleIvSize;
        }
        const subsamples = [];
        if (flags & 0x02) {
            if (off + 2 > data.length) break;
            const ssCount = data.readUInt16BE(off); off += 2;
            for (let j = 0; j < ssCount; j++) {
                if (off + 6 > data.length) break;
                subsamples.push([data.readUInt16BE(off), data.readUInt32BE(off + 2)]);
                off += 6;
            }
        }
        entries.push({ iv, subsamples });
    }
    return entries;
}

function parseMoofMdat(moofData, mdatData, defaults, audioTrackId, moofOffset, mdatDataOffset, perSampleIvSize) {
    const samples = [];
    const boxes = readBoxes(moofData, 8, moofData.length);

    for (const box of boxes) {
        if (box.type !== 'traf') continue;

        let tfhd = {
            trackId: 0,
            // tfhd descIndex=0 means "not set"; fall back to trex default
            descIndex: null,
            baseDataOffset: null,
            defaultDuration: defaults.defaultSampleDuration,
            defaultSize: defaults.defaultSampleSize,
        };
        let trunEntries = [];
        let firstTrunDataOffset = null;
        let sencEntries = [];

        const trafBoxes = readBoxes(box.data, 8, box.data.length);
        for (const tb of trafBoxes) {
            if (tb.type === 'tfhd') {
                const parsed = parseTfhd(tb.data.slice(8));
                if (parsed) {
                    tfhd.trackId = parsed.trackId;
                    // Only override descIndex if flag 0x02 was explicitly set in tfhd
                    if (parsed.descIndex !== 0) tfhd.descIndex = parsed.descIndex;
                    if (parsed.baseDataOffset !== null) tfhd.baseDataOffset = parsed.baseDataOffset;
                    if (parsed.defaultDuration !== null) tfhd.defaultDuration = parsed.defaultDuration;
                    if (parsed.defaultSize !== null) tfhd.defaultSize = parsed.defaultSize;
                }
            } else if (tb.type === 'trun') {
                const { entries, dataOffset } = parseTrun(tb.data.slice(8));
                if (firstTrunDataOffset === null && dataOffset !== null) firstTrunDataOffset = dataOffset;
                trunEntries.push(...entries);
            } else if (tb.type === 'senc') {
                sencEntries = parseSenc(tb.data.slice(8), perSampleIvSize);
            }
        }

        if (tfhd.trackId !== audioTrackId) continue;

        const base = tfhd.baseDataOffset !== null ? tfhd.baseDataOffset : moofOffset;
        let mdatIdx = firstTrunDataOffset !== null ? (base + firstTrunDataOffset) - mdatDataOffset : 0;
        let readOff = Math.max(0, mdatIdx);

        // Use explicit tfhd descIndex if set, else fall back to trex default_sample_description_index
        const rawDescIndex = tfhd.descIndex !== null ? tfhd.descIndex : defaults.defaultSampleDescIndex;
        const descIndex = rawDescIndex > 0 ? rawDescIndex - 1 : 0; // convert to 0-based

        for (let i = 0; i < trunEntries.length; i++) {
            const entry = trunEntries[i];
            const sampleSize = entry.size !== undefined ? entry.size : tfhd.defaultSize;
            const sampleDuration = entry.duration !== undefined ? entry.duration : tfhd.defaultDuration;
            if (sampleSize > 0 && readOff + sampleSize <= mdatData.length) {
                const sampleIv = i < sencEntries.length ? sencEntries[i].iv : Buffer.alloc(0);
                const sampleSubsamples = i < sencEntries.length ? sencEntries[i].subsamples : [];
                samples.push({
                    data: mdatData.slice(readOff, readOff + sampleSize),
                    duration: sampleDuration,
                    descIndex,
                    iv: sampleIv,
                    subsamples: sampleSubsamples,
                });
                readOff += sampleSize;
            }
        }
    }
    return samples;
}

// ── Sample decryption ────────────────────────────────────────────────────────

function decryptSamples(samples, keys, encInfoPerDesc) {
    const parts = [];
    for (const sample of samples) {
        const key = keys[sample.descIndex];
        if (!key) { parts.push(sample.data); continue; }

        const encInfo = (encInfoPerDesc && encInfoPerDesc[sample.descIndex]) ||
            { schemeType: 'cbcs', perSampleIvSize: 0, constantIv: Buffer.alloc(16) };
        const isCenc = encInfo.schemeType === 'cenc';

        if (isCenc) {
            // AES-128-CTR: per-sample IV from senc, zero-padded to 16 bytes
            let iv = sample.iv.length > 0 ? sample.iv : Buffer.alloc(8);
            if (iv.length < 16) { const p = Buffer.alloc(16); iv.copy(p); iv = p; }

            if (sample.subsamples.length > 0) {
                const plaintext = [];
                let off = 0;
                for (const [clearBytes, encBytes] of sample.subsamples) {
                    plaintext.push(sample.data.slice(off, off + clearBytes));
                    off += clearBytes;
                    if (encBytes > 0) {
                        const cipher = crypto.createDecipheriv('aes-128-ctr', key, iv);
                        cipher.setAutoPadding(false);
                        plaintext.push(Buffer.concat([cipher.update(sample.data.slice(off, off + encBytes)), cipher.final()]));
                    }
                    off += encBytes;
                }
                if (off < sample.data.length) plaintext.push(sample.data.slice(off));
                parts.push(Buffer.concat(plaintext));
            } else {
                const cipher = crypto.createDecipheriv('aes-128-ctr', key, iv);
                cipher.setAutoPadding(false);
                parts.push(Buffer.concat([cipher.update(sample.data), cipher.final()]));
            }
        } else {
            // AES-128-CBC (cbcs): constant IV from tenc (or per-sample fallback)
            let iv = sample.iv.length > 0 ? sample.iv : (encInfo.constantIv || Buffer.alloc(16));
            if (iv.length < 16) { const p = Buffer.alloc(16); iv.copy(p); iv = p; }

            if (sample.subsamples.length > 0) {
                // Collect all encrypted regions, decrypt as one CBC stream, then reassemble
                const encParts = [];
                const encSizes = [];
                let off = 0;
                for (const [clearBytes, encBytes] of sample.subsamples) {
                    off += clearBytes;
                    if (encBytes > 0) { encParts.push(sample.data.slice(off, off + encBytes)); encSizes.push(encBytes); }
                    off += encBytes;
                }

                let decConcat = Buffer.alloc(0);
                if (encParts.length > 0) {
                    const encConcat = Buffer.concat(encParts);
                    const cbcLen = encConcat.length & ~0xF;
                    if (cbcLen > 0) {
                        const cipher = crypto.createDecipheriv('aes-128-cbc', key, iv);
                        cipher.setAutoPadding(false);
                        const dec = Buffer.concat([cipher.update(encConcat.slice(0, cbcLen)), cipher.final()]);
                        decConcat = Buffer.concat([dec, encConcat.slice(cbcLen)]);
                    } else {
                        decConcat = encConcat;
                    }
                }

                const plaintext = [];
                let decOff = 0;
                off = 0;
                for (const [clearBytes, encBytes] of sample.subsamples) {
                    plaintext.push(sample.data.slice(off, off + clearBytes));
                    off += clearBytes;
                    if (encBytes > 0) { plaintext.push(decConcat.slice(decOff, decOff + encBytes)); decOff += encBytes; }
                    off += encBytes;
                }
                if (off < sample.data.length) plaintext.push(sample.data.slice(off));
                parts.push(Buffer.concat(plaintext));
            } else {
                const sampleLen = sample.data.length;
                const cbcLen = sampleLen & ~0xF;
                if (cbcLen === 0) {
                    parts.push(sample.data);
                } else {
                    const cipher = crypto.createDecipheriv('aes-128-cbc', key, iv);
                    cipher.setAutoPadding(false);
                    const dec = Buffer.concat([cipher.update(sample.data.slice(0, cbcLen)), cipher.final()]);
                    parts.push(cbcLen < sampleLen ? Buffer.concat([dec, sample.data.slice(cbcLen)]) : dec);
                }
            }
        }
    }
    return Buffer.concat(parts);
}

// ── Fragmented MP4 cleanup (remove senc, fix data_offset) ───────────────────

/**
 * Patch trun data_offset by `delta` bytes.
 * data_offset (flag 0x01) sits at byte 16 in the trun box (after size+type+ver+flags+sampleCount).
 */
function fixTrunDataOffset(trunBoxData, delta) {
    const flags = (trunBoxData[9] << 16) | (trunBoxData[10] << 8) | trunBoxData[11];
    if (!(flags & 0x01)) return trunBoxData; // no data_offset field — nothing to patch
    const out = Buffer.from(trunBoxData);
    out.writeInt32BE(out.readInt32BE(16) + delta, 16);
    return out;
}

/**
 * Remove senc boxes from a traf and fix trun data_offset to account for the
 * smaller moof (since senc was inside the moof's traf).
 */
function removeSencFromTraf(trafData) {
    const children = readBoxes(trafData, 8, trafData.length);
    let sencRemoved = 0;
    for (const box of children) {
        if (box.type === 'senc') sencRemoved += box.size;
    }
    const parts = [];
    for (const box of children) {
        if (box.type === 'senc') continue;
        if (box.type === 'trun' && sencRemoved > 0) {
            parts.push(fixTrunDataOffset(box.data, -sencRemoved));
        } else {
            parts.push(box.data);
        }
    }
    return makeBox('traf', Buffer.concat(parts));
}

function removeSencFromMoof(moofData) {
    const children = readBoxes(moofData, 8, moofData.length);
    const parts = [];
    for (const box of children) {
        parts.push(box.type === 'traf' ? removeSencFromTraf(box.data) : box.data);
    }
    return makeBox('moof', Buffer.concat(parts));
}

// ── Main entry point ─────────────────────────────────────────────────────────

/**
 * Decrypt an encrypted Apple Music CMAF buffer.
 * Returns a cleaned FRAGMENTED MP4 (ftyp + moov with mp4a stsd + decrypted moof/mdat pairs).
 * senc boxes are removed from moofs; trun data_offsets are corrected.
 * Feed the result to `ffmpeg -i pipe:0 -c:a copy -f adts pipe:1` for playback.
 *
 * @param {Buffer} encBuf       - Concatenated init segment + media segments (encrypted)
 * @param {string} trackKeyHex  - 32-char hex AES-128 key from Widevine CDM
 * @param {boolean} legacy      - true = cenc/CTR (legacy AAC), false = cbcs/CBC (enhanced HLS)
 * @returns {Buffer} Cleaned fragmented MP4 with decrypted audio
 */
function decryptMp4(encBuf, trackKeyHex, legacy) {
    // createDecryptState parses the entire buffer, decrypts all moof/mdat pairs,
    // and returns them folded into state.header (ftyp + cleanedMoov + decrypted pairs).
    const state = createDecryptState(encBuf, trackKeyHex, legacy);
    return state.header;
}

/**
 * Parse the init segment (ftyp + moov) and return a reusable decrypt state.
 * Call this once, then call decryptSegmentBuf() for each subsequent segment.
 *
 * @param {Buffer} initBuf      - The init segment (ftyp + moov boxes only)
 * @param {string} trackKeyHex  - 32-char hex track key from Widevine CDM
 * @param {boolean} legacy      - true = cenc/CTR, false = cbcs/CBC
 * @returns {object} state      - Pass to decryptSegmentBuf()
 */
function createDecryptState(initBuf, trackKeyHex, legacy) {
    const trackKey = Buffer.from(trackKeyHex, 'hex');
    const keys = legacy
        ? { 0: trackKey }
        : { 0: DEFAULT_SONG_DECRYPTION_KEY, 1: trackKey };

    const topBoxes = readBoxes(initBuf);
    let ftypData = null;
    let moovData = null;
    const pairs = [];
    let pendingMoof = null;

    for (const box of topBoxes) {
        if (box.type === 'ftyp') ftypData = box.data;
        else if (box.type === 'moov') moovData = box.data;
        else if (box.type === 'moof') pendingMoof = box;
        else if (box.type === 'mdat' && pendingMoof !== null) {
            pairs.push({ moof: pendingMoof, mdat: box });
            pendingMoof = null;
        }
    }

    if (!moovData) throw new Error('mp4decrypt: no moov box in init segment');

    const audioTrackId = getAudioTrackId(moovData);
    const trexDefaults = getTrexDefaults(moovData, audioTrackId);
    const encInfoPerDesc = extractEncInfoPerDesc(moovData);

    let perSampleIvSize = 0;
    if (legacy) {
        perSampleIvSize = 8;
    } else if (encInfoPerDesc) {
        const defaultDescIdx0based = trexDefaults.defaultSampleDescIndex > 0
            ? trexDefaults.defaultSampleDescIndex - 1 : 0;
        const ei = encInfoPerDesc[defaultDescIdx0based] || encInfoPerDesc[0] || encInfoPerDesc[1];
        if (ei) perSampleIvSize = ei.perSampleIvSize;
    }

    console.log(`[mp4decrypt] legacy=${legacy} audioTrackId=${audioTrackId} trexDescIdx=${trexDefaults.defaultSampleDescIndex} perSampleIvSize=${perSampleIvSize}`);

    const cleanedMoov = rebuildBox(moovData, { stsd: cleanStsdBox });
    const headerParts = [];
    if (ftypData) headerParts.push(ftypData);
    headerParts.push(cleanedMoov);

    const state = {
        keys,
        audioTrackId,
        trexDefaults,
        encInfoPerDesc,
        perSampleIvSize,
        _pairs: [], // unused in incremental path but kept for decryptMp4 compat
        _pairCount: 0,
    };

    // Decrypt any moof/mdat pairs embedded in the init buffer itself and fold them
    // into the header. Apple's EXT-X-MAP "init segment" often embeds the first chunk
    // of audio data alongside ftyp+moov, so these must be included before media segments.
    for (let i = 0; i < pairs.length; i++) {
        headerParts.push(decryptPair(state, pairs[i], i === 0));
        state._pairCount++;
    }

    state.header = Buffer.concat(headerParts);
    return state;
}

/** Decrypt one moof+mdat pair from a pre-parsed box list. Internal helper. */
function decryptPair(state, { moof, mdat }, isFirst) {
    const mdatContent = mdat.data.slice(mdat.hdrSize);
    const mdatDataOffset = mdat.offset + mdat.hdrSize;

    const samples = parseMoofMdat(
        moof.data, mdatContent, state.trexDefaults, state.audioTrackId,
        moof.offset, mdatDataOffset, state.perSampleIvSize
    );

    if (isFirst && samples.length > 0) {
        const s0 = samples[0];
        console.log(`  [mp4decrypt] seg0 sample0: descIndex=${s0.descIndex} size=${s0.data.length} iv=${s0.iv.toString('hex')} subsamples=${s0.subsamples.length}`);
    }

    const decryptedBuf = decryptSamples(samples, state.keys, state.encInfoPerDesc);
    const cleanedMoof = removeSencFromMoof(moof.data);
    const mdatHdr = Buffer.alloc(8);
    mdatHdr.writeUInt32BE(8 + decryptedBuf.length, 0);
    mdatHdr.write('mdat', 4, 'ascii');

    return Buffer.concat([cleanedMoof, mdatHdr, decryptedBuf]);
}

/**
 * Decrypt one media segment buffer (may contain one or more moof+mdat pairs).
 * The segment buffer offsets are self-contained (relative to the segment start).
 *
 * @param {object} state  - Created by createDecryptState()
 * @param {Buffer} segBuf - One segment's raw bytes (moof + mdat)
 * @returns {Buffer}      - Decrypted moof+mdat bytes, ready to pipe to player
 */
function decryptSegmentBuf(state, segBuf) {
    const boxes = readBoxes(segBuf);
    const parts = [];
    let pendingMoof = null;
    let isFirst = state._pairCount === 0;

    for (const box of boxes) {
        if (box.type === 'moof') {
            pendingMoof = box;
        } else if (box.type === 'mdat' && pendingMoof !== null) {
            parts.push(decryptPair(state, { moof: pendingMoof, mdat: box }, isFirst));
            state._pairCount++;
            isFirst = false;
            pendingMoof = null;
        }
    }

    return parts.length > 0 ? Buffer.concat(parts) : Buffer.alloc(0);
}

module.exports = { decryptMp4, createDecryptState, decryptSegmentBuf };
