use reqwest::Client;
use serde_json::{json, Value};

use crate::errors::MhResult;
use crate::services::common::ids::cpn;

const ORIGIN: &str = "https://music.youtube.com";
const PLAYER_URL: &str = "https://music.youtube.com/youtubei/v1/player?prettyPrint=false";
const IDENTITY: &str = "c=WEB_REMIX&cver=1.20260721.00.00&cbr=Chrome&cbrver=126.0.0.0&cos=X11&cplatform=DESKTOP&cplayer=UNIPLAYER&cr=US&hl=en_US&ver=2&fmt=0&fs=0&vis=1&muted=0&volume=100&idpj=-1&ldpj=-7";

fn append_params(base: &str, extra: &str) -> String {
    if base.contains('?') {
        format!("{base}&{extra}")
    } else {
        format!("{base}?{extra}")
    }
}

async fn beacon(client: &Client, auth: &str, cookie: &str, url: &str) {
    let _ = client
        .get(url)
        .header("authorization", auth)
        .header("cookie", cookie)
        .header("origin", ORIGIN)
        .header("x-origin", ORIGIN)
        .header("x-goog-authuser", "0")
        .send()
        .await;
}

pub async fn report_playback(
    client: &Client,
    auth: &str,
    cookie: &str,
    context: Value,
    video_id: &str,
    elapsed_secs: u64,
) -> MhResult<()> {
    let body = json!({ "videoId": video_id, "context": context });
    let resp = client
        .post(PLAYER_URL)
        .header("authorization", auth)
        .header("cookie", cookie)
        .header("origin", ORIGIN)
        .header("x-origin", ORIGIN)
        .header("x-goog-authuser", "0")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(crate::errors::MhError::Network)?;
    if !resp.status().is_success() {
        return Ok(());
    }
    let v: Value = resp.json().await.map_err(crate::errors::MhError::Network)?;
    let tracking = &v["playbackTracking"];
    let playback_url = tracking["videostatsPlaybackUrl"]["baseUrl"]
        .as_str()
        .unwrap_or("");
    let watchtime_url = tracking["videostatsWatchtimeUrl"]["baseUrl"]
        .as_str()
        .unwrap_or("");
    if playback_url.is_empty() || watchtime_url.is_empty() {
        return Ok(());
    }

    let atr_url = tracking["atrUrl"]["baseUrl"].as_str().unwrap_or("");
    let nonce = cpn();
    let total = elapsed_secs.max(1);

    beacon(
        client,
        auth,
        cookie,
        &append_params(
            playback_url,
            &format!(
                "{IDENTITY}&cpn={nonce}&cmt=0.02&rt=0.5&rtn=10&lact=64&el=detailpage&afmt=141"
            ),
        ),
    )
    .await;
    if !atr_url.is_empty() {
        beacon(
            client,
            auth,
            cookie,
            &append_params(atr_url, &format!("{IDENTITY}&cpn={nonce}&el=detailpage")),
        )
        .await;
    }

    let mut st = 0u64;
    while st < total {
        let et = (st + 10).min(total);
        let last = et >= total;
        let state = if last { "paused" } else { "playing" };
        beacon(
            client,
            auth,
            cookie,
            &append_params(
                watchtime_url,
                &format!(
                    "{IDENTITY}&cpn={nonce}&st={st}&et={et}&cmt={et}&state={state}&rt={et}&rtn={}&lact=500&el=detailpage&afmt=141",
                    et + 10
                ),
            ),
        )
        .await;
        st = et;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpn_is_16_url_safe_chars() {
        let c = cpn();
        assert_eq!(c.len(), 16);
        assert!(c
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_'));
    }

    #[test]
    fn append_params_respects_existing_query() {
        assert_eq!(
            append_params("https://x/api?a=1", "b=2"),
            "https://x/api?a=1&b=2"
        );
        assert_eq!(append_params("https://x/api", "b=2"), "https://x/api?b=2");
    }
}
