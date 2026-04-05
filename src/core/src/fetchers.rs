use scraper::{Html, Selector};
use url::Url;

use crate::errors::MhResult;
use crate::http_client::build_mozilla_client;

pub async fn fetch_website_title(url: &str) -> MhResult<String> {
    let client = build_mozilla_client()?;

    let html = match client.get(url).send().await {
        Ok(resp) => match resp.text().await {
            Ok(text) => text,
            Err(_) => return Ok("Unknown Title".to_string()),
        },
        Err(_) => return Ok("Unknown Title".to_string()),
    };

    let document = Html::parse_document(&html);
    let sel = Selector::parse("title").unwrap();

    let title = document
        .select(&sel)
        .next()
        .map(|el| el.text().collect::<String>().trim().to_string())
        .unwrap_or_default();

    if title.is_empty() {
        return Ok("Unknown Title".to_string());
    }

    let truncated = if title.chars().count() > 50 {
        let cut: String = title.chars().take(50).collect();
        format!("{}\u{2026}", cut) // … = U+2026
    } else {
        title
    };

    Ok(truncated)
}

pub fn extract_domain(url: &str) -> String {
    match Url::parse(url) {
        Ok(parsed) => {
            let host = parsed.host_str().unwrap_or(url);
            if host.starts_with("www.") {
                host[4..].to_string()
            } else {
                host.to_string()
            }
        }
        Err(_) => url.to_string(),
    }
}

pub async fn fetch_favicon_or_image(url: &str) -> MhResult<String> {
    let client = build_mozilla_client()?;

    let html = match client.get(url).send().await {
        Ok(resp) => match resp.text().await {
            Ok(text) => text,
            Err(_) => return Ok("/favicon.ico".to_string()),
        },
        Err(_) => return Ok("/favicon.ico".to_string()),
    };

    let base_origin = Url::parse(url)
        .map(|u| format!("{}://{}", u.scheme(), u.host_str().unwrap_or("")))
        .unwrap_or_default();

    let document = Html::parse_document(&html);

    let og_sel = Selector::parse(r#"meta[property="og:image"]"#).unwrap();
    if let Some(el) = document.select(&og_sel).next() {
        if let Some(content) = el.value().attr("content") {
            return Ok(resolve_url(content, &base_origin));
        }
    }

    let apple_sel = Selector::parse(r#"link[rel="apple-touch-icon"]"#).unwrap();
    if let Some(el) = document.select(&apple_sel).next() {
        if let Some(href) = el.value().attr("href") {
            return Ok(resolve_url(href, &base_origin));
        }
    }

    let icon_sizes_sel = Selector::parse(r#"link[rel="icon"][sizes]"#).unwrap();
    if let Some(el) = document.select(&icon_sizes_sel).next() {
        if let Some(href) = el.value().attr("href") {
            return Ok(resolve_url(href, &base_origin));
        }
    }

    let icon_sel = Selector::parse(r#"link[rel="icon"]"#).unwrap();
    if let Some(el) = document.select(&icon_sel).next() {
        if let Some(href) = el.value().attr("href") {
            return Ok(resolve_url(href, &base_origin));
        }
    }

    if base_origin.is_empty() {
        Ok("/favicon.ico".to_string())
    } else {
        Ok(format!("{}/favicon.ico", base_origin))
    }
}

fn resolve_url(href: &str, base_origin: &str) -> String {
    if href.starts_with("http://") || href.starts_with("https://") || href.starts_with("//") {
        href.to_string()
    } else if !base_origin.is_empty() {
        let slash = if href.starts_with('/') { "" } else { "/" };
        format!("{}{}{}", base_origin, slash, href)
    } else {
        href.to_string()
    }
}
