use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::services::radio::model::{Station, StreamKind};

/// The fields a user may change on a station that a directory also supplies.
///
/// Only changed fields are stored. `remember()` rewrites the descriptive columns
/// on every `resolve()` — which happens on every play — so an edit written into
/// those columns would be wiped the next time the station started. Keeping the
/// diff separate means a station whose bitrate is corrected upstream still picks
/// that up; only the field the user actually touched is frozen.
///
/// `cover_id` and `headers` are deliberately absent: no directory supplies them,
/// so they are plain columns with nothing to diverge from.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StationOverrides {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_kind: Option<StreamKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alt_urls: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub favicon: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub codec: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bitrate: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
}

/// An edit as the editor sends it: every field the form knows about, carrying
/// the value the user wants to see. What is actually stored is the diff against
/// the station the directory gave us.
///
/// It reuses [`StationOverrides`] for its shape rather than restating those
/// twelve fields, but the two mean different things — these are the values asked
/// for, those are the differences kept — so the type stays distinct and the
/// bundle is named. `flatten` keeps the wire shape flat, so the frontend sends
/// the same JSON either way.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StationEdit {
    #[serde(flatten)]
    pub fields: StationOverrides,
    /// Written straight to its column — a directory never supplies headers.
    pub headers: Option<BTreeMap<String, String>>,
}

/// An empty string clears an optional field; `None` on the station is what a
/// cleared field looks like everywhere else.
fn cleared(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

impl StationOverrides {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Rewrites `station` in place with whatever the user changed.
    pub fn apply(&self, station: &mut Station) {
        // Name and stream URL cannot be blank — a station with neither is not a
        // station, so an empty override for them is ignored rather than obeyed.
        if let Some(name) = self.name.as_deref().and_then(cleared) {
            station.name = name;
        }
        if let Some(url) = self.stream_url.as_deref().and_then(cleared) {
            station.stream_url = url;
        }
        if let Some(kind) = self.stream_kind {
            station.stream_kind = kind;
        }
        if let Some(urls) = &self.alt_urls {
            station.alt_urls = urls.iter().filter_map(|u| cleared(u)).collect();
        }
        for (slot, value) in [
            (&mut station.favicon, &self.favicon),
            (&mut station.tags, &self.tags),
            (&mut station.country, &self.country),
            (&mut station.country_code, &self.country_code),
            (&mut station.language, &self.language),
            (&mut station.codec, &self.codec),
            (&mut station.homepage, &self.homepage),
        ] {
            if let Some(v) = value {
                *slot = cleared(v);
            }
        }
        if let Some(bitrate) = self.bitrate {
            station.bitrate = (bitrate > 0).then_some(bitrate);
        }
    }

    /// The diff between what the directory gave us and what the user asked for.
    ///
    /// Setting a field back to the directory's own value is how "reset this
    /// field" works: the diff simply stops recording it.
    pub fn from_edit(base: &Station, edit: &StationEdit) -> Self {
        fn differs(edited: &Option<String>, base: Option<&str>) -> Option<String> {
            let edited = edited.as_deref()?.trim();
            let base = base.unwrap_or("").trim();
            (edited != base).then(|| edited.to_string())
        }

        let want = &edit.fields;
        Self {
            name: differs(&want.name, Some(&base.name)),
            stream_url: differs(&want.stream_url, Some(&base.stream_url)),
            stream_kind: want.stream_kind.filter(|k| *k != base.stream_kind),
            alt_urls: want.alt_urls.as_ref().and_then(|urls| {
                let cleaned: Vec<String> = urls.iter().filter_map(|u| cleared(u)).collect();
                (cleaned != base.alt_urls).then_some(cleaned)
            }),
            favicon: differs(&want.favicon, base.favicon.as_deref()),
            tags: differs(&want.tags, base.tags.as_deref()),
            country: differs(&want.country, base.country.as_deref()),
            country_code: differs(&want.country_code, base.country_code.as_deref()),
            language: differs(&want.language, base.language.as_deref()),
            codec: differs(&want.codec, base.codec.as_deref()),
            bitrate: want.bitrate.filter(|b| Some(*b) != base.bitrate),
            homepage: differs(&want.homepage, base.homepage.as_deref()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Station {
        let mut s = Station::new(
            "radiobrowser",
            "abc",
            "Directory Name".into(),
            "https://dir.example/stream".into(),
        );
        s.tags = Some("jazz".into());
        s.bitrate = Some(128);
        s.codec = Some("MP3".into());
        s
    }

    #[test]
    fn only_changed_fields_are_recorded() {
        let base = base();
        let edit = StationEdit {
            fields: StationOverrides {
                name: Some("My Name".into()),
                stream_url: Some(base.stream_url.clone()),
                tags: Some("jazz".into()),
                bitrate: Some(128),
                ..Default::default()
            },
            ..Default::default()
        };
        let ov = StationOverrides::from_edit(&base, &edit);
        assert_eq!(ov.name.as_deref(), Some("My Name"));
        assert_eq!(ov.stream_url, None, "unchanged URL must not be recorded");
        assert_eq!(ov.tags, None);
        assert_eq!(ov.bitrate, None);
    }

    /// The whole point: a field the user never touched keeps tracking upstream.
    #[test]
    fn a_directory_refresh_updates_untouched_fields_and_not_overridden_ones() {
        let ov = StationOverrides {
            name: Some("My Name".into()),
            ..Default::default()
        };

        let mut refreshed = base();
        refreshed.name = "Directory Renamed It".into();
        refreshed.bitrate = Some(320);
        ov.apply(&mut refreshed);

        assert_eq!(refreshed.name, "My Name", "the override wins");
        assert_eq!(
            refreshed.bitrate,
            Some(320),
            "everything else tracks upstream"
        );
    }

    #[test]
    fn setting_a_field_back_to_the_directory_value_clears_the_override() {
        let base = base();
        let edit = StationEdit {
            fields: StationOverrides {
                name: Some("Directory Name".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(StationOverrides::from_edit(&base, &edit).is_empty());
    }

    #[test]
    fn an_empty_string_clears_an_optional_field_but_never_the_name() {
        let ov = StationOverrides {
            name: Some("   ".into()),
            tags: Some(String::new()),
            ..Default::default()
        };
        let mut station = base();
        ov.apply(&mut station);
        assert_eq!(station.name, "Directory Name", "a blank name is ignored");
        assert_eq!(station.tags, None, "a blank tag list clears it");
    }

    /// `flatten` is what keeps the editor's request shape unchanged by the split
    /// between the edit and the diff, so it is pinned rather than assumed.
    #[test]
    fn an_edit_arrives_as_one_flat_object() {
        let edit: StationEdit =
            serde_json::from_str(r#"{"name":"N","streamUrl":"u","headers":{"Referer":"r"}}"#)
                .unwrap();
        assert_eq!(edit.fields.name.as_deref(), Some("N"));
        assert_eq!(edit.fields.stream_url.as_deref(), Some("u"));
        assert_eq!(edit.headers.unwrap().get("Referer").unwrap(), "r");
    }

    #[test]
    fn empty_overrides_serialize_to_an_empty_object() {
        let json = serde_json::to_string(&StationOverrides::default()).unwrap();
        assert_eq!(json, "{}");
        let back: StationOverrides = serde_json::from_str("{}").unwrap();
        assert!(back.is_empty());
    }
}
