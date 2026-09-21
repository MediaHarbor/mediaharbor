//! Generic, schema-validated option bag for codec (and container) init.
//!
//! The over-the-wire form is an untyped string→string bag
//! ([`CodecOptions`]). Each codec defines a typed struct implementing
//! [`CodecOptionsStruct`], which declares a static [`OptionField`]
//! schema and an [`apply`](CodecOptionsStruct::apply) method that
//! writes one coerced value into the struct. [`parse_options`] drives
//! the whole thing: it walks the bag, looks up every key in the
//! schema, coerces the string to the declared [`OptionKind`], and
//! hands the resulting [`OptionValue`] to `apply`.
//!
//! Strict at init: unknown keys and malformed values return
//! [`Error::InvalidData`]. Consumers that want "ignore unknown keys"
//! should pre-filter the bag before calling [`parse_options`].
//!
//! All parsing happens once, at encoder/decoder construction — the
//! hot path never touches this module.
//!
//! Consumers have two entry points:
//! - **Dynamic / JSON** — build a [`CodecOptions`] via `.set(k, v)` or
//!   [`CodecOptions::from_json`] (feature `json-options`) and attach
//!   it to `CodecParameters::options`.
//! - **Typed** — skip the bag entirely: build the codec's options
//!   struct directly and pass it to a codec-specific typed entry point
//!   (e.g. `encode_single_with_options`). The bag only exists for
//!   consumers who can't know the typed struct at compile time.

use crate::error::{Error, Result};

/// Untyped string → string bag. The over-the-wire shape of options
/// as they travel from the caller (CLI / pipeline JSON / FFI) to a
/// codec factory.
///
/// Insertion order is preserved and [`iter`](Self::iter) walks keys in
/// the order they were set. Duplicate keys overwrite (last writer
/// wins).
#[derive(Debug, Clone, Default)]
pub struct CodecOptions {
    entries: Vec<(String, String)>,
}

impl CodecOptions {
    /// Create an empty option bag (same as `CodecOptions::default()`).
    pub fn new() -> Self {
        Self::default()
    }

    /// Builder-style setter, useful for one-liners.
    /// `CodecOptions::new().set("interlace", "true")`.
    pub fn set(mut self, k: impl Into<String>, v: impl Into<String>) -> Self {
        self.insert(k, v);
        self
    }

    /// Mutating insert. Overwrites any existing entry with the same
    /// key.
    pub fn insert(&mut self, k: impl Into<String>, v: impl Into<String>) {
        let k = k.into();
        let v = v.into();
        if let Some(existing) = self.entries.iter_mut().find(|(kk, _)| kk == &k) {
            existing.1 = v;
        } else {
            self.entries.push((k, v));
        }
    }

    /// Look up the value for key `k`, or `None` if the key was never
    /// set.
    pub fn get(&self, k: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(kk, _)| kk == k)
            .map(|(_, v)| v.as_str())
    }

    /// `true` when the bag contains no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Number of entries in the bag.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Iterate over `(key, value)` pairs in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// Build a bag from a JSON object. Scalar values (bool / number /
    /// string) are stringified into the bag; arrays and nested objects
    /// are rejected — keys with structured values don't map into the
    /// flat string bag.
    pub fn from_json(s: &str) -> Result<Self> {
        let v: serde_json::Value =
            serde_json::from_str(s).map_err(|e| Error::invalid(format!("options json: {e}")))?;
        Self::from_json_value(&v)
    }

    /// As [`from_json`](Self::from_json) but takes a pre-parsed value
    /// (the shape pipelines already use — `TrackSpec.codec_params`).
    pub fn from_json_value(v: &serde_json::Value) -> Result<Self> {
        use serde_json::Value;
        let obj = match v {
            Value::Null => return Ok(Self::default()),
            Value::Object(m) => m,
            other => {
                return Err(Error::invalid(format!(
                    "options json: expected object, got {}",
                    json_type_name(other)
                )))
            }
        };
        let mut out = Self::default();
        for (k, val) in obj {
            let s = match val {
                Value::Bool(b) => b.to_string(),
                Value::Number(n) => n.to_string(),
                Value::String(s) => s.clone(),
                Value::Null => continue, // null = "leave default"
                other => {
                    return Err(Error::invalid(format!(
                        "option '{k}': structured values ({}) are not supported",
                        json_type_name(other)
                    )))
                }
            };
            out.insert(k.clone(), s);
        }
        Ok(out)
    }
}

fn json_type_name(v: &serde_json::Value) -> &'static str {
    use serde_json::Value;
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Declared type of a single option. Used at parse time to coerce a
/// raw string (or JSON scalar) into a typed [`OptionValue`] and to
/// reject malformed values up front.
#[derive(Clone, Copy, Debug)]
pub enum OptionKind {
    /// Boolean; accepts `true`/`1`/`yes`/`on` and `false`/`0`/`no`/`off`.
    Bool,
    /// Unsigned 32-bit integer.
    U32,
    /// Signed 32-bit integer.
    I32,
    /// 32-bit floating point.
    F32,
    /// Free-form string; any value is accepted verbatim.
    String,
    /// Enumeration: the only accepted values are the strings in this
    /// slice. Matching is case-sensitive.
    Enum(&'static [&'static str]),
}

/// Coerced value handed to a codec's `apply` method. Codec code
/// chooses the appropriate `as_*` accessor based on the field name.
#[derive(Clone, Debug)]
pub enum OptionValue {
    /// A coerced boolean value.
    Bool(bool),
    /// A coerced unsigned 32-bit integer.
    U32(u32),
    /// A coerced signed 32-bit integer.
    I32(i32),
    /// A coerced 32-bit float.
    F32(f32),
    /// A string value (also used for [`OptionKind::Enum`] matches).
    String(String),
}

impl OptionValue {
    /// Extract the boolean, or [`Error::InvalidData`] when the value is
    /// a different kind.
    pub fn as_bool(&self) -> Result<bool> {
        match self {
            OptionValue::Bool(b) => Ok(*b),
            other => Err(Error::invalid(format!("expected bool, got {other:?}"))),
        }
    }
    /// Extract the `u32`, or [`Error::InvalidData`] when the value is a
    /// different kind.
    pub fn as_u32(&self) -> Result<u32> {
        match self {
            OptionValue::U32(n) => Ok(*n),
            other => Err(Error::invalid(format!("expected u32, got {other:?}"))),
        }
    }
    /// Extract the `i32`, or [`Error::InvalidData`] when the value is a
    /// different kind.
    pub fn as_i32(&self) -> Result<i32> {
        match self {
            OptionValue::I32(n) => Ok(*n),
            other => Err(Error::invalid(format!("expected i32, got {other:?}"))),
        }
    }
    /// Extract the `f32`, or [`Error::InvalidData`] when the value is a
    /// different kind.
    pub fn as_f32(&self) -> Result<f32> {
        match self {
            OptionValue::F32(n) => Ok(*n),
            other => Err(Error::invalid(format!("expected f32, got {other:?}"))),
        }
    }
    /// Extract the string (also the shape of `Enum` matches), or
    /// [`Error::InvalidData`] when the value is a different kind.
    pub fn as_str(&self) -> Result<&str> {
        match self {
            OptionValue::String(s) => Ok(s.as_str()),
            other => Err(Error::invalid(format!("expected string, got {other:?}"))),
        }
    }
}

/// Schema entry describing one recognised option. Codec crates declare
/// a `&'static [OptionField]` listing every key their options struct
/// consumes.
#[derive(Debug)]
pub struct OptionField {
    /// Option key as it appears in the [`CodecOptions`] bag.
    pub name: &'static str,
    /// Declared type used to coerce and validate the raw string value.
    pub kind: OptionKind,
    /// Value used when the key is absent from the bag (documentation /
    /// introspection — the actual default lives in the struct's
    /// `Default` impl).
    pub default: OptionValue,
    /// One-line human-readable description for `--help`-style listings.
    pub help: &'static str,
}

/// Trait implemented by each codec's typed options struct.
///
/// Typical hand-written implementation:
///
/// ```ignore
/// impl CodecOptionsStruct for PngEncoderOptions {
///     const SCHEMA: &'static [OptionField] = &[
///         OptionField {
///             name: "interlace",
///             kind: OptionKind::Bool,
///             default: OptionValue::Bool(false),
///             help: "Adam7 interlaced encode",
///         },
///     ];
///     fn apply(&mut self, key: &str, v: &OptionValue) -> Result<()> {
///         match key {
///             "interlace" => self.interlace = v.as_bool()?,
///             _ => unreachable!("guarded by SCHEMA"),
///         }
///         Ok(())
///     }
/// }
/// ```
pub trait CodecOptionsStruct: Default + 'static {
    /// Static schema listing every option key this struct consumes,
    /// with its declared kind, default, and help text.
    const SCHEMA: &'static [OptionField];
    /// Write one coerced value into the struct. `key` is guaranteed to
    /// be present in [`SCHEMA`](Self::SCHEMA) and `value` to match the
    /// declared [`OptionKind`] when called via [`parse_options`].
    fn apply(&mut self, key: &str, value: &OptionValue) -> Result<()>;
}

/// Parse a [`CodecOptions`] bag into a typed options struct.
///
/// Strict: unknown keys return [`Error::InvalidData`]; malformed values
/// do the same. The returned struct is seeded from
/// `T::default()` — any key not set in the bag keeps the struct's
/// default value.
pub fn parse_options<T: CodecOptionsStruct>(opts: &CodecOptions) -> Result<T> {
    let mut out = T::default();
    for (k, v_str) in opts.iter() {
        let field = T::SCHEMA
            .iter()
            .find(|f| f.name == k)
            .ok_or_else(|| Error::invalid(format!("unknown option '{k}'")))?;
        let v = coerce(k, field.kind, v_str)?;
        out.apply(k, &v)?;
    }
    Ok(out)
}

/// Shorthand: parse straight from a JSON-object source.
pub fn parse_options_json<T: CodecOptionsStruct>(s: &str) -> Result<T> {
    parse_options::<T>(&CodecOptions::from_json(s)?)
}

fn coerce(name: &str, kind: OptionKind, raw: &str) -> Result<OptionValue> {
    match kind {
        OptionKind::Bool => match raw {
            "true" | "1" | "yes" | "on" => Ok(OptionValue::Bool(true)),
            "false" | "0" | "no" | "off" => Ok(OptionValue::Bool(false)),
            other => Err(Error::invalid(format!(
                "option '{name}' expects bool, got {other:?}"
            ))),
        },
        OptionKind::U32 => raw
            .parse::<u32>()
            .map(OptionValue::U32)
            .map_err(|_| Error::invalid(format!("option '{name}' expects u32, got {raw:?}"))),
        OptionKind::I32 => raw
            .parse::<i32>()
            .map(OptionValue::I32)
            .map_err(|_| Error::invalid(format!("option '{name}' expects i32, got {raw:?}"))),
        OptionKind::F32 => raw
            .parse::<f32>()
            .map(OptionValue::F32)
            .map_err(|_| Error::invalid(format!("option '{name}' expects f32, got {raw:?}"))),
        OptionKind::String => Ok(OptionValue::String(raw.to_owned())),
        OptionKind::Enum(allowed) => {
            if allowed.contains(&raw) {
                Ok(OptionValue::String(raw.to_owned()))
            } else {
                Err(Error::invalid(format!(
                    "option '{name}' must be one of {:?}, got {raw:?}",
                    allowed
                )))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default, Debug, PartialEq)]
    struct Demo {
        interlace: bool,
        level: u32,
        mode: String,
    }

    impl CodecOptionsStruct for Demo {
        const SCHEMA: &'static [OptionField] = &[
            OptionField {
                name: "interlace",
                kind: OptionKind::Bool,
                default: OptionValue::Bool(false),
                help: "",
            },
            OptionField {
                name: "level",
                kind: OptionKind::U32,
                default: OptionValue::U32(6),
                help: "",
            },
            OptionField {
                name: "mode",
                kind: OptionKind::Enum(&["fast", "slow"]),
                default: OptionValue::String(String::new()),
                help: "",
            },
        ];
        fn apply(&mut self, key: &str, v: &OptionValue) -> Result<()> {
            match key {
                "interlace" => self.interlace = v.as_bool()?,
                "level" => self.level = v.as_u32()?,
                "mode" => self.mode = v.as_str()?.to_owned(),
                _ => unreachable!("guarded by SCHEMA"),
            }
            Ok(())
        }
    }

    #[test]
    fn bag_preserves_order_and_overwrites() {
        let opts = CodecOptions::new()
            .set("a", "1")
            .set("b", "2")
            .set("a", "3");
        assert_eq!(opts.get("a"), Some("3"));
        let collected: Vec<_> = opts.iter().collect();
        assert_eq!(collected, vec![("a", "3"), ("b", "2")]);
    }

    #[test]
    fn parse_empty_returns_default() {
        let opts = CodecOptions::new();
        let d = parse_options::<Demo>(&opts).unwrap();
        assert_eq!(d, Demo::default());
    }

    #[test]
    fn parse_typed_values() {
        let opts = CodecOptions::new()
            .set("interlace", "true")
            .set("level", "9")
            .set("mode", "fast");
        let d = parse_options::<Demo>(&opts).unwrap();
        assert!(d.interlace);
        assert_eq!(d.level, 9);
        assert_eq!(d.mode, "fast");
    }

    #[test]
    fn parse_rejects_unknown_key() {
        let opts = CodecOptions::new().set("nope", "1");
        let err = parse_options::<Demo>(&opts).unwrap_err();
        assert!(matches!(err, Error::InvalidData(ref s) if s.contains("unknown option 'nope'")));
    }

    #[test]
    fn parse_rejects_bad_bool() {
        let opts = CodecOptions::new().set("interlace", "maybe");
        let err = parse_options::<Demo>(&opts).unwrap_err();
        assert!(matches!(err, Error::InvalidData(ref s) if s.contains("expects bool")));
    }

    #[test]
    fn parse_rejects_bad_u32() {
        let opts = CodecOptions::new().set("level", "-1");
        assert!(parse_options::<Demo>(&opts).is_err());
    }

    #[test]
    fn parse_rejects_enum_miss() {
        let opts = CodecOptions::new().set("mode", "medium");
        let err = parse_options::<Demo>(&opts).unwrap_err();
        assert!(matches!(err, Error::InvalidData(ref s) if s.contains("must be one of")));
    }

    #[test]
    fn bool_accepts_common_synonyms() {
        for (raw, want) in [
            ("true", true),
            ("1", true),
            ("yes", true),
            ("on", true),
            ("false", false),
            ("0", false),
            ("no", false),
            ("off", false),
        ] {
            let opts = CodecOptions::new().set("interlace", raw);
            let d = parse_options::<Demo>(&opts).unwrap();
            assert_eq!(d.interlace, want, "raw = {raw}");
        }
    }

    #[test]
    fn from_json_object() {
        let bag =
            CodecOptions::from_json(r#"{"interlace": true, "level": 9, "mode": "fast"}"#).unwrap();
        let d = parse_options::<Demo>(&bag).unwrap();
        assert!(d.interlace);
        assert_eq!(d.level, 9);
        assert_eq!(d.mode, "fast");
    }

    #[test]
    fn from_json_null_is_empty() {
        let bag = CodecOptions::from_json("null").unwrap();
        assert!(bag.is_empty());
    }

    #[test]
    fn from_json_rejects_nested() {
        let err = CodecOptions::from_json(r#"{"k": [1, 2]}"#).unwrap_err();
        assert!(matches!(err, Error::InvalidData(ref s) if s.contains("structured")));
    }

    #[test]
    fn parse_options_json_shortcut() {
        let d = parse_options_json::<Demo>(r#"{"level": 3}"#).unwrap();
        assert_eq!(d.level, 3);
    }
}
