use reqwest::Client;
use serde_json::json;

use crate::errors::MhResult;

const PLAY_URL: &str = "https://universal-activity-service.itunes.apple.com/play";
const ORIGIN: &str = "https://music.apple.com";

pub async fn report_playback(
    client: &Client,
    dev_token: &str,
    media_user_token: &str,
    store_front: &str,
    adam_id: &str,
    duration_ms: u64,
    feature_name: &str,
) -> MhResult<()> {
    let played_ms = duration_ms.max(1);
    for (event_type, extra) in [
        (
            "PLAY_START",
            json!({ "eventReasonHintType": "INITIAL_PLAY" }),
        ),
        (
            "PLAY_END",
            json!({
                "endReasonType": "NATURAL_END_OF_TRACK",
                "endTime": played_ms,
                "endPositionInMilliseconds": played_ms,
            }),
        ),
    ] {
        let mut record = json!({
            "adamId": adam_id,
            "contentType": "Song",
            "mediaType": 0,
            "startTime": 0,
            "startPositionInMilliseconds": 0,
            "mediaDurationInMilliseconds": duration_ms,
            "milliseconds-since-play": played_ms,
            "featureName": feature_name,
            "storeFront": store_front,
            "ids": { "subscription-adam-id": adam_id },
        });
        if let (Some(obj), Some(ex)) = (record.as_object_mut(), extra.as_object()) {
            for (k, v) in ex {
                obj.insert(k.clone(), v.clone());
            }
        }
        let body = json!({
            "client_id": "music",
            "event_type": event_type,
            "data": [record],
        });
        let _ = client
            .post(PLAY_URL)
            .header("authorization", format!("Bearer {dev_token}"))
            .header("media-user-token", media_user_token)
            .header("content-type", "application/json")
            .header("origin", ORIGIN)
            .json(&body)
            .send()
            .await;
    }
    Ok(())
}
