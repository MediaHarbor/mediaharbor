use std::sync::Arc;
use std::time::Duration;

use crate::ipc_contract::RadioMetadataEvent;
use crate::services::radio::directory::{
    xiph, CustomStations, RadioBrowser, SomaFm, XiphDirectory,
};
use crate::services::radio::icecast;
use crate::services::radio::import::origin_of;
use crate::services::radio::model::{Station, StreamKind};
use crate::streaming_server::StreamLiveness;
use crate::EventEmitter;

const POLL_INTERVAL: Duration = Duration::from_secs(20);

/// Where a station's titles can come from when they are not in the audio.
enum Source {
    /// A curated network that publishes what it is playing. The adapter is held
    /// rather than rebuilt, because this is polled every twenty seconds for as
    /// long as the station plays.
    SomaFm { client: SomaFm, id: String },
    /// Any Icecast or AzuraCast host, read from its standard status document.
    /// The two addresses are separate because they often are: the status host is
    /// where the station was imported from, the listen URL is what it streams.
    Icecast { status_host: String, listen: String },
    /// The Icecast public directory, which already carries a title per mount.
    Xiph(String),
}

/// Where to ask for a status document. An imported station kept the host it came
/// from in `homepage`; anything else can only guess at the stream's own origin.
fn status_host_for(station: &Station) -> Option<String> {
    station
        .homepage
        .clone()
        .filter(|h| h.starts_with("http"))
        .or_else(|| origin_of(&station.stream_url))
}

/// Picks an out-of-band title source, or `None` when the in-band ICY stream is
/// already the better answer.
///
/// HLS is the case this exists for: it carries no ICY at all, and the timed-ID3
/// that would carry titles is discarded by the `-c:a copy -f adts` remux. A
/// plain ICY station is left alone — [`crate::streaming_server`] parses its
/// titles out of the body, which is both cheaper and more timely.
fn source_for(station: &Station) -> Option<Source> {
    let icecast = || {
        Some(Source::Icecast {
            status_host: status_host_for(station)?,
            listen: station.stream_url.clone(),
        })
    };
    match station.source.as_str() {
        SomaFm::ID => Some(Source::SomaFm {
            client: SomaFm::new().ok()?,
            id: station.source_id.clone(),
        }),
        XiphDirectory::ID => Some(Source::Xiph(station.source_id.clone())),
        CustomStations::ID => icecast(),
        // Opportunistic, and it costs exactly one request to find out: a
        // directory station that has to be remuxed has no other title source.
        RadioBrowser::ID if station.stream_kind == StreamKind::Hls => icecast(),
        _ => None,
    }
}

async fn poll(source: &Source) -> Option<String> {
    match source {
        Source::SomaFm { client, id } => client.now_playing(id).await.ok().flatten(),
        Source::Icecast {
            status_host,
            listen,
        } => icecast::now_playing(status_host, listen)
            .await
            .ok()
            .flatten(),
        Source::Xiph(listen) => xiph::cached_now_playing(listen).await,
    }
}

/// Starts polling for now-playing titles, if this station has anywhere to poll.
///
/// The task's lifetime follows the stream's: it stops as soon as the entry
/// leaves the streaming server, which is what happens when another station is
/// played or the app shuts down. A host that does not answer is dropped after
/// the first failure rather than retried forever.
pub fn spawn(
    station: &Station,
    liveness: StreamLiveness,
    emitter: Arc<dyn EventEmitter>,
) -> Option<tokio::task::JoinHandle<()>> {
    let source = source_for(station)?;
    let station_key = station.key.clone();
    Some(tokio::spawn(async move {
        let mut last = String::new();
        let mut first = true;
        while liveness.is_live() {
            match poll(&source).await {
                Some(title) if title != last => {
                    last = title.clone();
                    emitter.emit_radio_metadata(&RadioMetadataEvent {
                        station_uuid: station_key.clone(),
                        title,
                    });
                }
                Some(_) => {}
                // Nothing at all on the first try means this host does not
                // publish titles; keeping the timer alive would only burn
                // requests against a stream that plays for hours. The Icecast
                // mirror is exempt: it costs nothing to re-read, and a title
                // appears there whenever the directory next refreshes.
                None if first && !matches!(source, Source::Xiph(_)) => return,
                None => {}
            }
            first = false;
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }))
}
