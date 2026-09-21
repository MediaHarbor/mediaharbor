//! Unified subtitle cue representation.
//!
//! Produced by subtitle-format decoders (SRT, WebVTT, ASS/SSA) and consumed
//! by the corresponding encoders. Timing is expressed in microseconds from
//! the start of the stream so the IR is format-independent.

/// A single displayable subtitle event.
#[derive(Clone, Debug, Default)]
pub struct SubtitleCue {
    /// Cue start, microseconds from stream start.
    pub start_us: i64,
    /// Cue end, microseconds from stream start.
    pub end_us: i64,
    /// Optional style name this cue inherits from. References an entry in
    /// the track-level style table (ASS `Style:` rows or WebVTT `::cue(.X)` rules).
    pub style_ref: Option<String>,
    /// Optional overriding position for this cue. `None` → use the style default.
    pub positioning: Option<CuePosition>,
    /// Cue body as a sequence of styled segments.
    pub segments: Vec<Segment>,
}

/// Positioning information for a cue.
///
/// Interpretation differs by source format:
/// * WebVTT — `x`/`y` are percentages of the viewport, `align` from cue settings.
/// * ASS `\pos(x, y)` — absolute pixel coordinates in the `PlayResX`×`PlayResY` canvas.
#[derive(Clone, Debug, Default)]
pub struct CuePosition {
    /// Horizontal position (WebVTT `position:N%` percentage, or ASS
    /// `\pos` pixel X). `None` → format default.
    pub x: Option<f32>,
    /// Vertical position (WebVTT `line:N%` percentage, or ASS `\pos`
    /// pixel Y). `None` → format default.
    pub y: Option<f32>,
    /// Horizontal text alignment for this cue.
    pub align: TextAlign,
    /// WebVTT `size:N%` cue setting. Irrelevant for ASS.
    pub size: Option<f32>,
}

/// Horizontal alignment for a cue / a style row.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextAlign {
    /// Aligned to the text-direction start edge (left in left-to-right
    /// scripts). WebVTT default.
    #[default]
    Start,
    /// Centered.
    Center,
    /// Aligned to the text-direction end edge (right in left-to-right
    /// scripts).
    End,
    /// Aligned to the left edge regardless of text direction.
    Left,
    /// Aligned to the right edge regardless of text direction.
    Right,
}

/// One inline element of a cue body.
#[derive(Clone, Debug)]
pub enum Segment {
    /// Plain text run.
    Text(String),
    /// Explicit line break (SRT/WebVTT newline, ASS `\N`).
    LineBreak,
    /// Bold-styled children (`<b>`, ASS `\b1`).
    Bold(Vec<Segment>),
    /// Italic-styled children (`<i>`, ASS `\i1`).
    Italic(Vec<Segment>),
    /// Underlined children (`<u>`, ASS `\u1`).
    Underline(Vec<Segment>),
    /// Strikethrough children (`<s>`, ASS `\s1`).
    Strike(Vec<Segment>),
    /// Children rendered in a specific text color (SRT `<font color>`,
    /// ASS `\c`).
    Color {
        /// Text color as an `(r, g, b)` triple, each channel `0..=255`.
        rgb: (u8, u8, u8),
        /// Segments the color applies to.
        children: Vec<Segment>,
    },
    /// Children rendered with a font override (SRT `<font>`, ASS
    /// `\fn` / `\fs`).
    Font {
        /// Font family name. `None` → inherit.
        family: Option<String>,
        /// Font size in the source format's units (ASS `\fs` points /
        /// `<font size>` value). `None` → inherit.
        size: Option<f32>,
        /// Segments the font override applies to.
        children: Vec<Segment>,
    },
    /// WebVTT `<v Speaker>...</v>`.
    Voice {
        /// Speaker name (the `<v>` annotation).
        name: String,
        /// Segments spoken by this voice.
        children: Vec<Segment>,
    },
    /// WebVTT `<c.classname>...</c>`.
    Class {
        /// CSS class name (without the leading dot).
        name: String,
        /// Segments the class applies to.
        children: Vec<Segment>,
    },
    /// ASS `{\k<cs>}` — the following text is highlighted for `cs` centiseconds.
    /// The children slice is the text under this karaoke beat (until the next
    /// `\k` override).
    Karaoke {
        /// Beat duration in centiseconds (the `\k` argument).
        cs: u32,
        /// Segments highlighted during this beat.
        children: Vec<Segment>,
    },
    /// WebVTT inline timestamp `<00:00:01.500>`.
    Timestamp {
        /// Timestamp value, microseconds from stream start.
        offset_us: i64,
    },
    /// Fallback for override tags we don't model explicitly. Carries the
    /// textual source verbatim so a re-emit to the same format stays faithful.
    Raw(String),
}

/// A named style definition — reusable across many cues.
#[derive(Clone, Debug, Default)]
pub struct SubtitleStyle {
    /// Style name that cues reference via [`SubtitleCue::style_ref`]
    /// (ASS `Style:` name or WebVTT cue class).
    pub name: String,
    /// Font family name. `None` → renderer default.
    pub font_family: Option<String>,
    /// Font size in the source format's units (ASS `Fontsize` points).
    /// `None` → renderer default.
    pub font_size: Option<f32>,
    /// Main text fill color as `(r, g, b, a)`, each channel `0..=255`.
    /// `None` → renderer default.
    pub primary_color: Option<(u8, u8, u8, u8)>,
    /// Text outline (border) color as `(r, g, b, a)`. `None` → default.
    pub outline_color: Option<(u8, u8, u8, u8)>,
    /// Background / shadow color as `(r, g, b, a)` (ASS `BackColour`).
    /// `None` → default.
    pub back_color: Option<(u8, u8, u8, u8)>,
    /// Bold text.
    pub bold: bool,
    /// Italic text.
    pub italic: bool,
    /// Underlined text.
    pub underline: bool,
    /// Strikethrough text.
    pub strike: bool,
    /// Horizontal text alignment.
    pub align: TextAlign,
    /// Left margin in pixels (ASS `MarginL`). `None` → default.
    pub margin_l: Option<i32>,
    /// Right margin in pixels (ASS `MarginR`). `None` → default.
    pub margin_r: Option<i32>,
    /// Vertical margin in pixels (ASS `MarginV`). `None` → default.
    pub margin_v: Option<i32>,
    /// Outline (border) thickness in pixels (ASS `Outline`).
    /// `None` → default.
    pub outline: Option<f32>,
    /// Drop-shadow offset in pixels (ASS `Shadow`). `None` → default.
    pub shadow: Option<f32>,
}

impl SubtitleStyle {
    /// Build a style with the given name and every other field at its
    /// default (`None` / `false` / [`TextAlign::Start`]).
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Default::default()
        }
    }
}
