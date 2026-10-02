use async_trait::async_trait;

use crate::defaults::Settings;
use crate::errors::{MhError, MhResult};
use crate::ipc_contract;
use crate::services::common::playback::PlaybackProvider;
use crate::services::radio::directory::{self, RadioBrowser};
use crate::services::radio::model::{split_key, Station, StreamKind};
use crate::services::radio::nowplaying;
use crate::services::radio::store::RadioStore;

/// How many radio streams may sit in the shared pool at once: the one playing,
/// the one prefetched behind it, and the one about to be registered.
const MAX_LIVE_RADIO_STREAMS: usize = 3;

pub struct RadioPlayback;

#[async_trait]
impl PlaybackProvider for RadioPlayback {
    async fn play(
        &self,
        req: &ipc_contract::PlayMediaRequest,
        _settings: &Settings,
        state: &crate::BackendState,
    ) -> MhResult<ipc_contract::PlayMediaResponse> {
        let key = req.url.trim();
        if key.is_empty() {
            return Err(MhError::NotFound("no station given".into()));
        }
        // A queue persisted before qualified keys existed still holds bare
        // radio-browser uuids, and they were all radio-browser's.
        let (source, source_id) = split_key(key).unwrap_or((RadioBrowser::ID, key));
        let qualified = crate::services::radio::model::station_key(source, source_id);

        let library = state.library.clone();
        let stored = RadioStore::get(library.db(), &qualified)?;
        let directory = directory::build(source, &library);

        let station = match &directory {
            Some(dir) => match dir.resolve(source_id).await {
                Ok(Some(fresh)) => {
                    RadioStore::remember(library.db(), &fresh)?;
                    Some(fresh)
                }
                // The directory being unreachable should not stop a favourite
                // from playing.
                _ => stored,
            },
            None => stored,
        };

        let Some(station) = station else {
            return Err(MhError::NotFound(format!(
                "station {qualified} is not in the directory or in your favourites"
            )));
        };
        if station.stream_url.is_empty() {
            return Err(MhError::NotFound(format!(
                "{} has no stream URL",
                station.name
            )));
        }

        let server = state.streaming_server()?;
        // Radio entries were never removed, so browsing eight stations filled the
        // shared eight-slot pool and evicted whatever else was playing. Two are
        // kept rather than one: the player prefetches the next queue entry while
        // the current station is still being served, so dropping all but the
        // newest would pull the stream out from under the listener.
        server.trim_streams_with_prefix("radio-", MAX_LIVE_RADIO_STREAMS - 1);

        let id = stream_id(&station);
        let url = match station.stream_kind {
            StreamKind::Hls => server.register_remux(
                &id,
                &station.stream_url,
                "audio/aac",
                station.headers.clone(),
            ),
            _ => server.register_radio(
                &id,
                &station.stream_url,
                &qualified,
                "audio/mpeg",
                station.headers.clone(),
            ),
        };

        RadioStore::record_play(library.db(), &qualified)?;
        nowplaying::spawn(&station, server.liveness(&id), state.emitter.clone());

        if let Some(dir) = directory {
            let source_id = source_id.to_string();
            tokio::spawn(async move { dir.report_play(&source_id).await });
        }

        Ok(ipc_contract::PlayMediaResponse::new(
            url, "radio", "audio", true,
        ))
    }
}

/// A URL-safe stream id. A custom station's id is the stream URL the user
/// pasted, colons and slashes included, so it cannot go into a path segment as
/// it stands; FNV-1a over the qualified key keeps it short, stable and safe.
///
/// Hand-rolled rather than `DefaultHasher` on purpose: std makes no guarantee
/// that its hasher produces the same value across releases, and a stream id that
/// changes under the running player is exactly what this must not do.
fn stream_id(station: &Station) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in station.key.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("radio-{}-{hash:016x}", station.source)
}
