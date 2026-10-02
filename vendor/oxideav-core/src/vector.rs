//! Vector graphics frame and primitive types.
//!
//! This module models a resolution-independent, scene-graph-style vector
//! frame so the same [`VectorFrame`] can round-trip through both SVG 1.1
//! and PDF 1.4 without lossy conversion. The primitive set is the
//! intersection of what those two formats represent natively:
//!
//! * paths built from move / line / quadratic / cubic / elliptic-arc / close
//!   commands,
//! * solid + linear-gradient + radial-gradient paints,
//! * stroke style (width, cap, join, miter limit, dash),
//! * even-odd / non-zero fill rules,
//! * 2D affine transforms,
//! * group nodes (transform, opacity, optional clip),
//! * embedded raster passthrough via [`ImageRef`] (carries a child
//!   [`VideoFrame`](crate::VideoFrame) — the rasterizer paints the image
//!   into vector space).
//!
//! Text nodes are intentionally **deferred to round 2** — text needs
//! font handling and tight scribe coupling that will land alongside the
//! `oxideav-svg` parser (#349). Round 1 is shape-only.
//!
//! No rasterizer / SVG parser / PDF writer lives in `oxideav-core`; those
//! are downstream tasks (#349 / #350 / #351). This module ships only the
//! data types every consumer of the vector pipeline needs to agree on.

use crate::time::TimeBase;

/// A decoded vector-graphics frame.
///
/// The `width` / `height` define the natural rendering canvas size in
/// user units. `view_box` lets a producer separate the user-coordinate
/// system from the canvas (an SVG `viewBox` attribute, or the PDF
/// `MediaBox` vs. `CropBox`); when `None`, callers should treat it as
/// `(0, 0, width, height)`.
#[derive(Clone, Debug)]
pub struct VectorFrame {
    /// Viewport width in user units.
    pub width: f32,
    /// Viewport height in user units.
    pub height: f32,
    /// Optional view box. `None` defaults to `(0, 0, width, height)`.
    pub view_box: Option<ViewBox>,
    /// Root group of the scene.
    pub root: Group,
    /// Presentation timestamp in `time_base` units, or `None` if unknown.
    pub pts: Option<i64>,
    /// Time base for `pts`. Consumers that don't care about timing
    /// (e.g. a one-shot SVG render) can use `TimeBase::new(1, 1)`.
    pub time_base: TimeBase,
}

impl VectorFrame {
    /// Build a `VectorFrame` of the given canvas size with an empty root
    /// group, no view box, no timestamp, and a `1/1` time base.
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            width,
            height,
            view_box: None,
            root: Group::default(),
            pts: None,
            time_base: TimeBase::new(1, 1),
        }
    }

    /// Replace the view box.
    pub fn with_view_box(mut self, view_box: ViewBox) -> Self {
        self.view_box = Some(view_box);
        self
    }

    /// Replace the root group.
    pub fn with_root(mut self, root: Group) -> Self {
        self.root = root;
        self
    }

    /// Set the presentation timestamp (in `time_base` units).
    pub fn with_pts(mut self, pts: i64) -> Self {
        self.pts = Some(pts);
        self
    }

    /// Replace the time base.
    pub fn with_time_base(mut self, time_base: TimeBase) -> Self {
        self.time_base = time_base;
        self
    }
}

impl Default for VectorFrame {
    /// An empty 0×0 frame with an empty root group, no view box, no
    /// timestamp, and a `1/1` time base. Useful as a starting point for
    /// builder-style construction or as a placeholder in
    /// `std::mem::take`-style swaps.
    fn default() -> Self {
        Self::new(0.0, 0.0)
    }
}

/// User-coordinate system rectangle. Mirrors the SVG `viewBox` attribute
/// and the PDF `MediaBox` / `CropBox` rectangles.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewBox {
    /// Left edge of the user-coordinate rectangle.
    pub min_x: f32,
    /// Top edge of the user-coordinate rectangle.
    pub min_y: f32,
    /// Width of the user-coordinate rectangle, in user units.
    pub width: f32,
    /// Height of the user-coordinate rectangle, in user units.
    pub height: f32,
}

impl ViewBox {
    /// Build a `ViewBox` from its origin and size.
    pub const fn new(min_x: f32, min_y: f32, width: f32, height: f32) -> Self {
        Self {
            min_x,
            min_y,
            width,
            height,
        }
    }
}

/// One node in the scene tree.
///
/// Marked `#[non_exhaustive]` so future variants (text, filters) can
/// be added without breaking downstream `match` arms.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum Node {
    /// A drawn path with optional fill and stroke.
    Path(PathNode),
    /// A nested group applying transform / opacity / clip to its children.
    Group(Group),
    /// An embedded raster image painted into vector space.
    Image(ImageRef),
    /// A soft-mask composite. The `mask` subtree is rasterised and
    /// converted to a per-pixel alpha multiplier (luminance or alpha,
    /// per [`MaskKind`]), then applied to the rasterised `content`
    /// subtree. Mirrors SVG `<mask>` and PDF `SMask` (subtype `Luminosity`
    /// vs. `Alpha`).
    SoftMask {
        /// Subtree rasterised to produce the per-pixel opacity
        /// modulator.
        mask: Box<Node>,
        /// How to convert the rasterised mask to a coverage value.
        mask_kind: MaskKind,
        /// Subtree whose pixels are modulated by the mask.
        content: Box<Node>,
    },
}

/// How to interpret a soft mask's rasterised pixels as a coverage
/// modulator.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MaskKind {
    /// Convert the mask's RGB to luminance (ITU-R BT.709 coefficients
    /// — Y = 0.2126·R + 0.7152·G + 0.0722·B) and use Y as the
    /// per-pixel alpha multiplier. Matches SVG `<mask>` default
    /// (`mask-type="luminance"`) and PDF `SMask` `/Luminosity`.
    #[default]
    Luminance,
    /// Use the mask's own alpha channel as the multiplier. Matches
    /// SVG `<mask mask-type="alpha">` and PDF `SMask` `/Alpha`.
    Alpha,
}

/// A grouping node — applies a transform / opacity / optional clip path
/// to all descendants. Mirrors SVG `<g>` and PDF `q ... Q` graphic-state
/// blocks.
#[derive(Clone, Debug)]
pub struct Group {
    /// Coordinate transform applied to children. Identity by default.
    pub transform: Transform2D,
    /// Group opacity in `0.0..=1.0`. `1.0` is fully opaque.
    pub opacity: f32,
    /// Optional clip path. Children are clipped to this path's interior
    /// (using the path's own fill rule). `None` means "no clip".
    pub clip: Option<Path>,
    /// Child nodes, painted in order (later children over earlier ones).
    pub children: Vec<Node>,
    /// Opaque cache key. When `Some(k)`, a downstream rasterizer is free
    /// to memoise the rendered bitmap of this group's content (after
    /// `transform` is applied) under key `k`, so re-rendering the same
    /// group at the same effective resolution returns the cached bitmap.
    ///
    /// Producers that emit cacheable content (e.g. scribe shaping a
    /// glyph at `(face_id, glyph_id, size_q8, subpixel_x)`) compute a
    /// deterministic hash of their identity tuple and put it here. The
    /// rasterizer treats it as a black box — `oxideav-core` never
    /// inspects the value, so each producer's namespace stays private.
    ///
    /// `None` (the default) means "do not cache; render fresh every
    /// time". Most synthesised vector content (a one-off rectangle, a
    /// gradient panel) leaves this `None`.
    pub cache_key: Option<u64>,
}

impl Default for Group {
    fn default() -> Self {
        Self {
            transform: Transform2D::identity(),
            opacity: 1.0,
            clip: None,
            children: Vec::new(),
            cache_key: None,
        }
    }
}

impl Group {
    /// An empty group: identity transform, opacity `1.0`, no clip, no
    /// children, no cache key. Same as [`Group::default`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace the transform.
    pub fn with_transform(mut self, transform: Transform2D) -> Self {
        self.transform = transform;
        self
    }

    /// Set the group opacity in `0.0..=1.0`.
    pub fn with_opacity(mut self, opacity: f32) -> Self {
        self.opacity = opacity;
        self
    }

    /// Set the clip path.
    pub fn with_clip(mut self, clip: Path) -> Self {
        self.clip = Some(clip);
        self
    }

    /// Append a child node.
    pub fn with_child(mut self, child: Node) -> Self {
        self.children.push(child);
        self
    }

    /// Replace the children list wholesale.
    pub fn with_children(mut self, children: Vec<Node>) -> Self {
        self.children = children;
        self
    }

    /// Set the rasterizer cache key. See [`Group::cache_key`].
    pub fn with_cache_key(mut self, key: u64) -> Self {
        self.cache_key = Some(key);
        self
    }
}

/// A drawn path with optional fill and stroke.
///
/// SVG `<path>` and PDF path-painting operators (`f`, `S`, `B`, `f*`,
/// `B*`) both express "one path, optional fill, optional stroke", so a
/// single struct covers both formats. At least one of `fill` / `stroke`
/// would normally be `Some` to produce visible output.
#[derive(Clone, Debug)]
pub struct PathNode {
    /// Path geometry, in the local user space of the enclosing group.
    pub path: Path,
    /// Paint for the path interior. `None` means "not filled".
    pub fill: Option<Paint>,
    /// Stroke style for the path outline. `None` means "not stroked".
    pub stroke: Option<Stroke>,
    /// Fill rule used for `fill` (and for hit-testing the interior).
    pub fill_rule: FillRule,
}

impl PathNode {
    /// Build a `PathNode` with `path`, no fill, no stroke, and
    /// `FillRule::NonZero`.
    pub fn new(path: Path) -> Self {
        Self {
            path,
            fill: None,
            stroke: None,
            fill_rule: FillRule::NonZero,
        }
    }

    /// Set the fill paint.
    pub fn with_fill(mut self, fill: Paint) -> Self {
        self.fill = Some(fill);
        self
    }

    /// Set the stroke style.
    pub fn with_stroke(mut self, stroke: Stroke) -> Self {
        self.stroke = Some(stroke);
        self
    }

    /// Set the fill rule.
    pub fn with_fill_rule(mut self, fill_rule: FillRule) -> Self {
        self.fill_rule = fill_rule;
        self
    }
}

/// A geometric path expressed as a sequence of drawing commands.
///
/// All coordinates are in the local user space of the enclosing group.
#[derive(Clone, Debug, Default)]
pub struct Path {
    /// Drawing commands, executed in order.
    pub commands: Vec<PathCommand>,
}

impl Path {
    /// An empty path with no commands.
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a [`PathCommand::MoveTo`] — start a new subpath at `p`.
    pub fn move_to(&mut self, p: Point) -> &mut Self {
        self.commands.push(PathCommand::MoveTo(p));
        self
    }

    /// Append a [`PathCommand::LineTo`] — straight line to `p`.
    pub fn line_to(&mut self, p: Point) -> &mut Self {
        self.commands.push(PathCommand::LineTo(p));
        self
    }

    /// Append a [`PathCommand::QuadCurveTo`] — quadratic Bezier to `end`
    /// with control point `control`.
    pub fn quad_to(&mut self, control: Point, end: Point) -> &mut Self {
        self.commands
            .push(PathCommand::QuadCurveTo { control, end });
        self
    }

    /// Append a [`PathCommand::CubicCurveTo`] — cubic Bezier to `end`
    /// with control points `c1` and `c2`.
    pub fn cubic_to(&mut self, c1: Point, c2: Point, end: Point) -> &mut Self {
        self.commands
            .push(PathCommand::CubicCurveTo { c1, c2, end });
        self
    }

    /// Append a [`PathCommand::Close`] — close the current subpath.
    pub fn close(&mut self) -> &mut Self {
        self.commands.push(PathCommand::Close);
        self
    }
}

/// A single path-construction command.
///
/// Marked `#[non_exhaustive]` so smooth-curve / Bezier-shorthand
/// variants can be added later without breaking match arms.
///
/// Note on `ArcTo`: SVG and PDF both accept elliptic-arc segments in
/// their path syntax (SVG `A` command, PDF via cubic approximation in
/// the writer). We keep the variant in the round-1 IR — converting an
/// arc to its spec-correct cubic-Bezier flattening is a pure function
/// of the arc parameters that downstream rasterizers / writers can do
/// independently.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum PathCommand {
    /// Start a new subpath at the given point (SVG `M`).
    MoveTo(Point),
    /// Straight line from the current point to the given point (SVG `L`).
    LineTo(Point),
    /// Quadratic Bezier segment from the current point (SVG `Q`).
    QuadCurveTo {
        /// The single quadratic control point.
        control: Point,
        /// Segment end point.
        end: Point,
    },
    /// Cubic Bezier segment from the current point (SVG `C`).
    CubicCurveTo {
        /// First control point (attached to the segment start).
        c1: Point,
        /// Second control point (attached to the segment end).
        c2: Point,
        /// Segment end point.
        end: Point,
    },
    /// SVG `A`-style elliptic arc segment. `x_axis_rot` is in radians
    /// (consistent with `Transform2D::rotate`); `large_arc` / `sweep`
    /// match the SVG flag semantics.
    ArcTo {
        /// Ellipse radius along its X axis, in user units.
        rx: f32,
        /// Ellipse radius along its Y axis, in user units.
        ry: f32,
        /// Rotation of the ellipse's X axis relative to the user-space
        /// X axis, in radians.
        x_axis_rot: f32,
        /// When `true`, pick the arc sweep of 180° or more (SVG
        /// `large-arc-flag`).
        large_arc: bool,
        /// When `true`, draw the arc in the positive-angle direction
        /// (SVG `sweep-flag`).
        sweep: bool,
        /// Arc end point.
        end: Point,
    },
    /// Close the current subpath with a straight line back to its
    /// starting point (SVG `Z`).
    Close,
}

/// 2D point in user-space coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    /// Horizontal coordinate in user units.
    pub x: f32,
    /// Vertical coordinate in user units (Y grows downward, per the
    /// SVG / PDF device-space convention used throughout this module).
    pub y: f32,
}

impl Point {
    /// Build a point from its coordinates.
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

impl From<[f32; 2]> for Point {
    fn from([x, y]: [f32; 2]) -> Self {
        Self { x, y }
    }
}

impl From<(f32, f32)> for Point {
    fn from((x, y): (f32, f32)) -> Self {
        Self { x, y }
    }
}

/// A paint server — what fills the inside of a path or strokes its
/// outline. The variant set is the SVG/PDF intersection.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum Paint {
    /// A single flat RGBA color.
    Solid(Rgba),
    /// Color stops swept along a straight line.
    LinearGradient(LinearGradient),
    /// Color stops swept outward from a focal point to a circle.
    RadialGradient(RadialGradient),
}

/// 32-bit straight (non-premultiplied) RGBA color.
///
/// Matches SVG's `rgb()` + `opacity` model and PDF's `RGB` + `CA`/`ca`
/// graphic-state model. Premultiplication is a rasterizer concern; this
/// IR carries straight alpha to avoid lossy round-trips.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rgba {
    /// Red channel, `0..=255`.
    pub r: u8,
    /// Green channel, `0..=255`.
    pub g: u8,
    /// Blue channel, `0..=255`.
    pub b: u8,
    /// Straight (non-premultiplied) alpha, `0` transparent to `255` opaque.
    pub a: u8,
}

impl Rgba {
    /// Build a color from its four channels (straight alpha).
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Fully-opaque color with the given RGB triple.
    pub const fn opaque(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }
}

impl From<(u8, u8, u8, u8)> for Rgba {
    fn from((r, g, b, a): (u8, u8, u8, u8)) -> Self {
        Self { r, g, b, a }
    }
}

impl From<(u8, u8, u8)> for Rgba {
    /// Fully-opaque color with the given RGB triple.
    fn from((r, g, b): (u8, u8, u8)) -> Self {
        Self { r, g, b, a: 255 }
    }
}

impl From<[u8; 4]> for Rgba {
    fn from([r, g, b, a]: [u8; 4]) -> Self {
        Self { r, g, b, a }
    }
}

impl From<Rgba> for Paint {
    /// Wrap an [`Rgba`] in a `Paint::Solid`.
    fn from(color: Rgba) -> Self {
        Paint::Solid(color)
    }
}

/// A linear gradient: color stops sweep along the line `start` → `end`.
#[derive(Clone, Debug)]
pub struct LinearGradient {
    /// Gradient axis start point (offset `0.0`), in user space.
    pub start: Point,
    /// Gradient axis end point (offset `1.0`), in user space.
    pub end: Point,
    /// Color stops, ordered by ascending `offset`.
    pub stops: Vec<GradientStop>,
    /// What to paint past the axis endpoints.
    pub spread: SpreadMethod,
}

impl LinearGradient {
    /// Build a `LinearGradient` from `start` → `end` with no stops and
    /// `SpreadMethod::Pad`.
    pub fn new(start: Point, end: Point) -> Self {
        Self {
            start,
            end,
            stops: Vec::new(),
            spread: SpreadMethod::Pad,
        }
    }

    /// Replace the gradient stops.
    pub fn with_stops(mut self, stops: Vec<GradientStop>) -> Self {
        self.stops = stops;
        self
    }

    /// Append a single stop.
    pub fn with_stop(mut self, stop: GradientStop) -> Self {
        self.stops.push(stop);
        self
    }

    /// Set the spread method.
    pub fn with_spread(mut self, spread: SpreadMethod) -> Self {
        self.spread = spread;
        self
    }
}

/// A radial gradient: color stops sweep from `focal` outward to a
/// circle of radius `radius` centered on `center`. When `focal` is
/// `None`, it defaults to `center` (the common case).
#[derive(Clone, Debug)]
pub struct RadialGradient {
    /// Center of the outer circle (offset `1.0`), in user space.
    pub center: Point,
    /// Radius of the outer circle, in user units.
    pub radius: f32,
    /// Focal point the stops sweep outward from (offset `0.0`).
    /// `None` defaults to `center`.
    pub focal: Option<Point>,
    /// Color stops, ordered by ascending `offset`.
    pub stops: Vec<GradientStop>,
    /// What to paint outside the outer circle.
    pub spread: SpreadMethod,
}

impl RadialGradient {
    /// Build a `RadialGradient` centered at `center` with `radius`, no
    /// focal point, no stops, and `SpreadMethod::Pad`.
    pub fn new(center: Point, radius: f32) -> Self {
        Self {
            center,
            radius,
            focal: None,
            stops: Vec::new(),
            spread: SpreadMethod::Pad,
        }
    }

    /// Set the focal point (defaults to `center` when `None`).
    pub fn with_focal(mut self, focal: Point) -> Self {
        self.focal = Some(focal);
        self
    }

    /// Replace the gradient stops.
    pub fn with_stops(mut self, stops: Vec<GradientStop>) -> Self {
        self.stops = stops;
        self
    }

    /// Append a single stop.
    pub fn with_stop(mut self, stop: GradientStop) -> Self {
        self.stops.push(stop);
        self
    }

    /// Set the spread method.
    pub fn with_spread(mut self, spread: SpreadMethod) -> Self {
        self.spread = spread;
        self
    }
}

/// One color stop along a gradient. `offset` is in `0.0..=1.0`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GradientStop {
    /// Position of the stop along the gradient axis. `0.0` is the
    /// start, `1.0` is the end.
    pub offset: f32,
    /// Color at this stop.
    pub color: Rgba,
}

impl GradientStop {
    /// Build a stop at `offset` (`0.0..=1.0`) with the given color.
    pub const fn new(offset: f32, color: Rgba) -> Self {
        Self { offset, color }
    }
}

/// What happens past the gradient endpoints. Mirrors SVG
/// `spreadMethod="pad|reflect|repeat"` and PDF gradient `Extend` arrays.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SpreadMethod {
    /// Final stop colors extend forever. SVG default.
    #[default]
    Pad,
    /// Gradient mirrors at each boundary.
    Reflect,
    /// Gradient repeats periodically.
    Repeat,
}

/// Stroke style for a path's outline.
#[derive(Clone, Debug)]
pub struct Stroke {
    /// Stroke width in user units, centered on the path.
    pub width: f32,
    /// Paint applied to the stroked outline.
    pub paint: Paint,
    /// How open-subpath endpoints are drawn.
    pub cap: LineCap,
    /// How segment corners are drawn.
    pub join: LineJoin,
    /// Miter limit ratio. SVG / PDF default is `4.0`.
    pub miter_limit: f32,
    /// Optional dash pattern. `None` means a solid (undashed) stroke.
    pub dash: Option<DashPattern>,
}

impl Stroke {
    /// Build a default solid-paint stroke with width `width`.
    pub fn solid(width: f32, color: Rgba) -> Self {
        Self {
            width,
            paint: Paint::Solid(color),
            cap: LineCap::Butt,
            join: LineJoin::Miter,
            miter_limit: 4.0,
            dash: None,
        }
    }

    /// Build a stroke with the given `width` and `paint`, and SVG/PDF
    /// default cap (`Butt`), join (`Miter`), miter limit (`4.0`), and
    /// no dash pattern.
    pub fn new(width: f32, paint: Paint) -> Self {
        Self {
            width,
            paint,
            cap: LineCap::Butt,
            join: LineJoin::Miter,
            miter_limit: 4.0,
            dash: None,
        }
    }

    /// Replace the stroke paint.
    pub fn with_paint(mut self, paint: Paint) -> Self {
        self.paint = paint;
        self
    }

    /// Set the line cap style.
    pub fn with_cap(mut self, cap: LineCap) -> Self {
        self.cap = cap;
        self
    }

    /// Set the line join style.
    pub fn with_join(mut self, join: LineJoin) -> Self {
        self.join = join;
        self
    }

    /// Set the miter limit ratio (SVG/PDF default is `4.0`).
    pub fn with_miter_limit(mut self, miter_limit: f32) -> Self {
        self.miter_limit = miter_limit;
        self
    }

    /// Set the dash pattern.
    pub fn with_dash(mut self, dash: DashPattern) -> Self {
        self.dash = Some(dash);
        self
    }
}

/// How an open path's endpoints are drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LineCap {
    /// Squared-off end flush with the endpoint. SVG / PDF default.
    #[default]
    Butt,
    /// Semicircular end of radius `width / 2` centered on the endpoint.
    Round,
    /// Squared-off end extending `width / 2` past the endpoint.
    Square,
}

/// How two stroke segments meet at a corner.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LineJoin {
    /// Sharp corner extended to a point, subject to the miter limit.
    /// SVG / PDF default.
    #[default]
    Miter,
    /// Corner rounded with a circular arc of radius `width / 2`.
    Round,
    /// Corner cut off with a straight edge across the outer angle.
    Bevel,
}

/// Dash pattern for a stroke. `array` is an alternating
/// dash-on / dash-off length list (in user units); `offset` is the
/// phase offset from the path start.
#[derive(Clone, Debug, Default)]
pub struct DashPattern {
    /// Alternating dash-on / dash-off lengths, in user units. An empty
    /// array means a solid stroke.
    pub array: Vec<f32>,
    /// Phase offset from the path start, in user units.
    pub offset: f32,
}

impl DashPattern {
    /// Build a dash pattern with the given lengths and a `0.0` phase
    /// offset.
    pub fn new(array: Vec<f32>) -> Self {
        Self { array, offset: 0.0 }
    }

    /// Set the phase offset from the path start.
    pub fn with_offset(mut self, offset: f32) -> Self {
        self.offset = offset;
        self
    }
}

/// Fill rule for self-intersecting and compound paths. Matches SVG's
/// `fill-rule` attribute and PDF's `f` (non-zero) vs. `f*` (even-odd)
/// painting operators.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FillRule {
    /// A point is inside when the winding number of the path around it
    /// is non-zero. SVG `fill-rule="nonzero"` (the default) / PDF `f`.
    #[default]
    NonZero,
    /// A point is inside when a ray from it crosses the path an odd
    /// number of times. SVG `fill-rule="evenodd"` / PDF `f*`.
    EvenOdd,
}

/// A 2D affine transform stored as the column-major matrix
///
/// ```text
/// | a c e |   | x |
/// | b d f | * | y |
/// | 0 0 1 |   | 1 |
/// ```
///
/// — i.e. `(x', y') = (a*x + c*y + e, b*x + d*y + f)`. The layout
/// matches SVG's `matrix(a, b, c, d, e, f)` and PDF's `cm` operator
/// argument order, so emitters can serialize fields directly.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform2D {
    /// X-scale term: contribution of input `x` to output `x`.
    pub a: f32,
    /// Y-skew term: contribution of input `x` to output `y`.
    pub b: f32,
    /// X-skew term: contribution of input `y` to output `x`.
    pub c: f32,
    /// Y-scale term: contribution of input `y` to output `y`.
    pub d: f32,
    /// X translation, in user units.
    pub e: f32,
    /// Y translation, in user units.
    pub f: f32,
}

impl Transform2D {
    /// The identity transform. `compose(identity, x) == x`.
    pub const fn identity() -> Self {
        Self {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            e: 0.0,
            f: 0.0,
        }
    }

    /// Build a translation by `(tx, ty)`.
    pub const fn translate(tx: f32, ty: f32) -> Self {
        Self {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            e: tx,
            f: ty,
        }
    }

    /// Build a non-uniform scale by `(sx, sy)` about the origin.
    pub const fn scale(sx: f32, sy: f32) -> Self {
        Self {
            a: sx,
            b: 0.0,
            c: 0.0,
            d: sy,
            e: 0.0,
            f: 0.0,
        }
    }

    /// Build a rotation by `angle_radians` about the origin
    /// (counter-clockwise in a Y-up system, clockwise visually under
    /// the SVG / PDF Y-down convention — this matches both formats).
    pub fn rotate(angle_radians: f32) -> Self {
        let (s, c) = angle_radians.sin_cos();
        Self {
            a: c,
            b: s,
            c: -s,
            d: c,
            e: 0.0,
            f: 0.0,
        }
    }

    /// Build a horizontal skew (shear along X) by `angle_radians`.
    pub fn skew_x(angle_radians: f32) -> Self {
        Self {
            a: 1.0,
            b: 0.0,
            c: angle_radians.tan(),
            d: 1.0,
            e: 0.0,
            f: 0.0,
        }
    }

    /// Build a vertical skew (shear along Y) by `angle_radians`.
    pub fn skew_y(angle_radians: f32) -> Self {
        Self {
            a: 1.0,
            b: angle_radians.tan(),
            c: 0.0,
            d: 1.0,
            e: 0.0,
            f: 0.0,
        }
    }

    /// Compose `self ∘ other` — the resulting transform applies
    /// `other` first, then `self`, to a point. Equivalent to
    /// `self.matrix() * other.matrix()` in column-vector form.
    pub fn compose(&self, other: &Self) -> Self {
        Self {
            a: self.a * other.a + self.c * other.b,
            b: self.b * other.a + self.d * other.b,
            c: self.a * other.c + self.c * other.d,
            d: self.b * other.c + self.d * other.d,
            e: self.a * other.e + self.c * other.f + self.e,
            f: self.b * other.e + self.d * other.f + self.f,
        }
    }

    /// Apply this transform to a point.
    pub fn apply(&self, p: Point) -> Point {
        Point {
            x: self.a * p.x + self.c * p.y + self.e,
            y: self.b * p.x + self.d * p.y + self.f,
        }
    }

    /// `true` when this transform is bit-identical to the identity.
    /// Useful for emitters that want to skip a no-op `matrix(...)` /
    /// `cm` write.
    pub fn is_identity(&self) -> bool {
        *self == Self::identity()
    }
}

impl Default for Transform2D {
    fn default() -> Self {
        Self::identity()
    }
}

/// An embedded raster image painted into vector space.
///
/// `bounds` is the axis-aligned rectangle (in the local user space,
/// before `transform`) that the image is painted into; SVG `<image>`
/// `x/y/width/height` and PDF `Do` with a matrix-pre-positioned
/// `Image` XObject both reduce to this shape.
#[derive(Clone, Debug)]
pub struct ImageRef {
    /// Embedded raster payload. Boxed so a `Node::Image` variant
    /// doesn't bloat every other [`Node`] case.
    pub frame: Box<crate::VideoFrame>,
    /// Destination rectangle the image is scaled into, in the local
    /// user space (before `transform`).
    pub bounds: Rect,
    /// Additional transform applied to the placed image, on top of the
    /// enclosing group's transform.
    pub transform: Transform2D,
}

/// Axis-aligned rectangle in user-space coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Rectangle width, in user units.
    pub width: f32,
    /// Rectangle height, in user units.
    pub height: f32,
}

impl Rect {
    /// Build a rectangle from its top-left corner and size.
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::TimeBase;

    fn approx_point(a: Point, b: Point) -> bool {
        (a.x - b.x).abs() < 1e-5 && (a.y - b.y).abs() < 1e-5
    }

    #[test]
    fn path_builder_produces_command_sequence() {
        let mut p = Path::new();
        p.move_to(Point::new(0.0, 0.0))
            .line_to(Point::new(10.0, 0.0))
            .quad_to(Point::new(15.0, 5.0), Point::new(10.0, 10.0))
            .cubic_to(
                Point::new(5.0, 15.0),
                Point::new(0.0, 10.0),
                Point::new(0.0, 0.0),
            )
            .close();
        assert_eq!(p.commands.len(), 5);
        assert_eq!(p.commands[0], PathCommand::MoveTo(Point::new(0.0, 0.0)));
        assert_eq!(p.commands[4], PathCommand::Close);
    }

    #[test]
    fn transform_identity_round_trips() {
        let id = Transform2D::identity();
        assert!(id.is_identity());
        let p = Point::new(3.5, -2.25);
        assert_eq!(id.apply(p), p);
    }

    #[test]
    fn transform_translate_round_trip() {
        let t = Transform2D::translate(10.0, -5.0);
        assert_eq!(t.apply(Point::new(0.0, 0.0)), Point::new(10.0, -5.0));
        assert_eq!(t.apply(Point::new(1.0, 1.0)), Point::new(11.0, -4.0));
    }

    #[test]
    fn transform_scale_round_trip() {
        let s = Transform2D::scale(2.0, 3.0);
        assert_eq!(s.apply(Point::new(1.0, 1.0)), Point::new(2.0, 3.0));
        assert_eq!(s.apply(Point::new(0.0, 0.0)), Point::new(0.0, 0.0));
    }

    #[test]
    fn transform_rotate_quarter_turn() {
        let r = Transform2D::rotate(std::f32::consts::FRAC_PI_2);
        // Under SVG/PDF Y-down with matrix(c,s,-s,c,0,0):
        // (1, 0) rotates to (cos, sin) = (0, 1).
        assert!(approx_point(
            r.apply(Point::new(1.0, 0.0)),
            Point::new(0.0, 1.0)
        ));
        // (0, 1) rotates to (-sin, cos) = (-1, 0).
        assert!(approx_point(
            r.apply(Point::new(0.0, 1.0)),
            Point::new(-1.0, 0.0)
        ));
    }

    #[test]
    fn transform_compose_identity_is_left_and_right_unit() {
        let t = Transform2D::translate(7.0, 11.0);
        let id = Transform2D::identity();
        assert_eq!(id.compose(&t), t);
        assert_eq!(t.compose(&id), t);
    }

    #[test]
    fn transform_compose_translate_then_scale() {
        // Apply translate(2,3) first, then scale(10,10):
        //   p -> p + (2,3) -> 10*(p+(2,3)) = 10p + (20,30).
        let scale = Transform2D::scale(10.0, 10.0);
        let translate = Transform2D::translate(2.0, 3.0);
        let composed = scale.compose(&translate);
        let result = composed.apply(Point::new(1.0, 1.0));
        assert!(approx_point(result, Point::new(30.0, 40.0)));
    }

    #[test]
    fn transform_compose_matches_sequential_apply() {
        // Composition equivalence: composed.apply(p) == a.apply(b.apply(p)).
        let a = Transform2D::rotate(0.5);
        let b = Transform2D::translate(3.0, -1.0);
        let composed = a.compose(&b);
        let p = Point::new(2.0, 5.0);
        let direct = composed.apply(p);
        let stepwise = a.apply(b.apply(p));
        assert!(approx_point(direct, stepwise));
    }

    #[test]
    fn group_default_is_identity_opacity_one_no_clip() {
        let g = Group::default();
        assert!(g.transform.is_identity());
        assert_eq!(g.opacity, 1.0);
        assert!(g.clip.is_none());
        assert!(g.children.is_empty());
    }

    #[test]
    fn group_nesting_with_transforms() {
        // Outer group translates by (10, 10); inner group scales by 2.
        // A point (1, 1) drawn at the inner level should land at
        // (12, 12) after the outer transform is also applied — but the
        // tree itself only stores the local transforms. This test
        // pins down that the nested data is preserved verbatim, since
        // composing transforms is a rasterizer responsibility.
        let inner = Group {
            transform: Transform2D::scale(2.0, 2.0),
            children: vec![Node::Path(PathNode {
                path: {
                    let mut p = Path::new();
                    p.move_to(Point::new(1.0, 1.0));
                    p
                },
                fill: Some(Paint::Solid(Rgba::opaque(255, 0, 0))),
                stroke: None,
                fill_rule: FillRule::NonZero,
            })],
            ..Group::default()
        };
        let outer = Group {
            transform: Transform2D::translate(10.0, 10.0),
            children: vec![Node::Group(inner)],
            ..Group::default()
        };
        match &outer.children[0] {
            Node::Group(g) => {
                assert_eq!(g.transform, Transform2D::scale(2.0, 2.0));
                assert_eq!(g.children.len(), 1);
            }
            _ => panic!("expected a Group child"),
        }
        assert_eq!(outer.transform, Transform2D::translate(10.0, 10.0));
    }

    #[test]
    fn vector_frame_construction() {
        let frame = VectorFrame {
            width: 100.0,
            height: 50.0,
            view_box: Some(ViewBox {
                min_x: 0.0,
                min_y: 0.0,
                width: 100.0,
                height: 50.0,
            }),
            root: Group::default(),
            pts: Some(0),
            time_base: TimeBase::new(1, 1000),
        };
        assert_eq!(frame.width, 100.0);
        assert_eq!(frame.height, 50.0);
        assert!(frame.view_box.is_some());
        assert_eq!(frame.pts, Some(0));
    }

    #[test]
    fn rgba_constructors() {
        let c = Rgba::opaque(10, 20, 30);
        assert_eq!(c.a, 255);
        let c2 = Rgba::new(10, 20, 30, 128);
        assert_eq!(c2.a, 128);
    }

    #[test]
    fn gradient_stop_round_trips() {
        let s = GradientStop::new(0.5, Rgba::opaque(255, 0, 0));
        assert_eq!(s.offset, 0.5);
        let s2 = GradientStop::new(0.5, Rgba::opaque(255, 0, 0));
        assert_eq!(s, s2);
    }

    #[test]
    fn stroke_solid_defaults() {
        let s = Stroke::solid(2.0, Rgba::opaque(0, 0, 0));
        assert_eq!(s.width, 2.0);
        assert_eq!(s.cap, LineCap::Butt);
        assert_eq!(s.join, LineJoin::Miter);
        assert_eq!(s.miter_limit, 4.0);
        assert!(s.dash.is_none());
    }

    #[test]
    fn soft_mask_construction_and_inspection() {
        // Wrap a path in a SoftMask node with a luminance mask. Round-
        // trips both children verbatim through clone + match.
        fn rect_path() -> PathNode {
            let mut p = Path::new();
            p.move_to(Point::new(0.0, 0.0))
                .line_to(Point::new(10.0, 0.0))
                .line_to(Point::new(10.0, 10.0))
                .line_to(Point::new(0.0, 10.0))
                .close();
            PathNode {
                path: p,
                fill: Some(Paint::Solid(Rgba::opaque(255, 255, 255))),
                stroke: None,
                fill_rule: FillRule::NonZero,
            }
        }
        let n = Node::SoftMask {
            mask: Box::new(Node::Path(rect_path())),
            mask_kind: MaskKind::Luminance,
            content: Box::new(Node::Path(rect_path())),
        };
        match &n {
            Node::SoftMask {
                mask_kind, content, ..
            } => {
                assert_eq!(*mask_kind, MaskKind::Luminance);
                match content.as_ref() {
                    Node::Path(_) => {}
                    _ => panic!("expected Path content"),
                }
            }
            _ => panic!("expected SoftMask"),
        }
    }

    #[test]
    fn mask_kind_default_is_luminance() {
        assert_eq!(MaskKind::default(), MaskKind::Luminance);
    }

    #[test]
    fn vector_frame_default_is_empty_zero_size() {
        let f = VectorFrame::default();
        assert_eq!(f.width, 0.0);
        assert_eq!(f.height, 0.0);
        assert!(f.view_box.is_none());
        assert!(f.root.children.is_empty());
        assert!(f.pts.is_none());
        assert_eq!(f.time_base, TimeBase::new(1, 1));
    }

    #[test]
    fn vector_frame_new_sets_canvas_size() {
        let f = VectorFrame::new(640.0, 480.0);
        assert_eq!(f.width, 640.0);
        assert_eq!(f.height, 480.0);
        assert!(f.view_box.is_none());
        assert!(f.root.children.is_empty());
        assert!(f.pts.is_none());
    }

    #[test]
    fn vector_frame_builder_chain() {
        let vb = ViewBox::new(0.0, 0.0, 100.0, 100.0);
        let f = VectorFrame::new(100.0, 100.0)
            .with_view_box(vb)
            .with_pts(42)
            .with_time_base(TimeBase::new(1, 90_000));
        assert_eq!(f.view_box, Some(vb));
        assert_eq!(f.pts, Some(42));
        assert_eq!(f.time_base, TimeBase::new(1, 90_000));
    }

    #[test]
    fn vector_frame_with_root_replaces_root() {
        let root = Group::new().with_opacity(0.5);
        let f = VectorFrame::new(10.0, 10.0).with_root(root);
        assert_eq!(f.root.opacity, 0.5);
    }

    #[test]
    fn view_box_new_round_trips_fields() {
        let vb = ViewBox::new(1.0, 2.0, 3.0, 4.0);
        assert_eq!(vb.min_x, 1.0);
        assert_eq!(vb.min_y, 2.0);
        assert_eq!(vb.width, 3.0);
        assert_eq!(vb.height, 4.0);
    }

    #[test]
    fn rect_new_round_trips_fields() {
        let r = Rect::new(1.0, 2.0, 3.0, 4.0);
        assert_eq!(r.x, 1.0);
        assert_eq!(r.y, 2.0);
        assert_eq!(r.width, 3.0);
        assert_eq!(r.height, 4.0);
    }

    #[test]
    fn group_new_matches_default() {
        let a = Group::new();
        let b = Group::default();
        assert!(a.transform.is_identity());
        assert_eq!(a.opacity, b.opacity);
        assert!(a.clip.is_none());
        assert_eq!(a.children.len(), b.children.len());
        assert_eq!(a.cache_key, b.cache_key);
    }

    #[test]
    fn group_builder_chain() {
        let mut clip = Path::new();
        clip.move_to(Point::new(0.0, 0.0))
            .line_to(Point::new(1.0, 1.0))
            .close();
        let g = Group::new()
            .with_transform(Transform2D::translate(5.0, 7.0))
            .with_opacity(0.25)
            .with_clip(clip)
            .with_cache_key(0xdead_beef);
        assert_eq!(g.transform, Transform2D::translate(5.0, 7.0));
        assert_eq!(g.opacity, 0.25);
        assert!(g.clip.is_some());
        assert_eq!(g.cache_key, Some(0xdead_beef));
    }

    #[test]
    fn group_with_child_appends() {
        let g = Group::new()
            .with_child(Node::Group(Group::new()))
            .with_child(Node::Group(Group::new().with_opacity(0.5)));
        assert_eq!(g.children.len(), 2);
        match &g.children[1] {
            Node::Group(inner) => assert_eq!(inner.opacity, 0.5),
            _ => panic!("expected Group child"),
        }
    }

    #[test]
    fn group_with_children_replaces_list() {
        let g = Group::new()
            .with_child(Node::Group(Group::new()))
            .with_children(vec![Node::Group(Group::new().with_opacity(0.1))]);
        assert_eq!(g.children.len(), 1);
        match &g.children[0] {
            Node::Group(inner) => assert_eq!(inner.opacity, 0.1),
            _ => panic!("expected Group child"),
        }
    }

    #[test]
    fn path_node_new_then_builder() {
        let mut p = Path::new();
        p.move_to(Point::new(0.0, 0.0))
            .line_to(Point::new(10.0, 0.0));
        let n = PathNode::new(p)
            .with_fill(Paint::Solid(Rgba::opaque(255, 0, 0)))
            .with_stroke(Stroke::solid(1.0, Rgba::opaque(0, 0, 0)))
            .with_fill_rule(FillRule::EvenOdd);
        assert!(n.fill.is_some());
        assert!(n.stroke.is_some());
        assert_eq!(n.fill_rule, FillRule::EvenOdd);
    }

    #[test]
    fn path_node_new_defaults() {
        let n = PathNode::new(Path::new());
        assert!(n.fill.is_none());
        assert!(n.stroke.is_none());
        assert_eq!(n.fill_rule, FillRule::NonZero);
    }

    #[test]
    fn point_from_array_and_tuple() {
        let p1: Point = [1.0_f32, 2.0_f32].into();
        let p2: Point = (3.0_f32, 4.0_f32).into();
        assert_eq!(p1, Point::new(1.0, 2.0));
        assert_eq!(p2, Point::new(3.0, 4.0));
    }

    #[test]
    fn rgba_from_tuples_and_array() {
        let a: Rgba = (10u8, 20u8, 30u8, 40u8).into();
        let b: Rgba = (50u8, 60u8, 70u8).into();
        let c: Rgba = [1u8, 2u8, 3u8, 4u8].into();
        assert_eq!(a, Rgba::new(10, 20, 30, 40));
        assert_eq!(b, Rgba::opaque(50, 60, 70));
        assert_eq!(c, Rgba::new(1, 2, 3, 4));
    }

    #[test]
    fn paint_from_rgba_wraps_solid() {
        let p: Paint = Rgba::opaque(1, 2, 3).into();
        match p {
            Paint::Solid(c) => assert_eq!(c, Rgba::opaque(1, 2, 3)),
            _ => panic!("expected Paint::Solid"),
        }
    }

    #[test]
    fn linear_gradient_new_then_builder() {
        let g = LinearGradient::new(Point::new(0.0, 0.0), Point::new(1.0, 0.0))
            .with_stop(GradientStop::new(0.0, Rgba::opaque(0, 0, 0)))
            .with_stop(GradientStop::new(1.0, Rgba::opaque(255, 255, 255)))
            .with_spread(SpreadMethod::Reflect);
        assert_eq!(g.start, Point::new(0.0, 0.0));
        assert_eq!(g.end, Point::new(1.0, 0.0));
        assert_eq!(g.stops.len(), 2);
        assert_eq!(g.spread, SpreadMethod::Reflect);
    }

    #[test]
    fn linear_gradient_with_stops_replaces() {
        let g = LinearGradient::new(Point::new(0.0, 0.0), Point::new(1.0, 0.0))
            .with_stop(GradientStop::new(0.5, Rgba::opaque(0, 0, 0)))
            .with_stops(vec![GradientStop::new(0.0, Rgba::opaque(1, 1, 1))]);
        assert_eq!(g.stops.len(), 1);
        assert_eq!(g.stops[0].offset, 0.0);
    }

    #[test]
    fn radial_gradient_new_then_builder() {
        let g = RadialGradient::new(Point::new(5.0, 5.0), 10.0)
            .with_focal(Point::new(4.0, 4.0))
            .with_stop(GradientStop::new(0.0, Rgba::opaque(0, 0, 0)))
            .with_spread(SpreadMethod::Repeat);
        assert_eq!(g.center, Point::new(5.0, 5.0));
        assert_eq!(g.radius, 10.0);
        assert_eq!(g.focal, Some(Point::new(4.0, 4.0)));
        assert_eq!(g.stops.len(), 1);
        assert_eq!(g.spread, SpreadMethod::Repeat);
    }

    #[test]
    fn radial_gradient_with_stops_replaces() {
        let g = RadialGradient::new(Point::new(0.0, 0.0), 1.0)
            .with_stop(GradientStop::new(0.5, Rgba::opaque(0, 0, 0)))
            .with_stops(vec![GradientStop::new(1.0, Rgba::opaque(1, 1, 1))]);
        assert_eq!(g.stops.len(), 1);
        assert_eq!(g.stops[0].offset, 1.0);
    }

    #[test]
    fn stroke_new_defaults() {
        let s = Stroke::new(3.0, Paint::Solid(Rgba::opaque(0, 0, 0)));
        assert_eq!(s.width, 3.0);
        assert_eq!(s.cap, LineCap::Butt);
        assert_eq!(s.join, LineJoin::Miter);
        assert_eq!(s.miter_limit, 4.0);
        assert!(s.dash.is_none());
    }

    #[test]
    fn stroke_builder_chain() {
        let s = Stroke::solid(1.0, Rgba::opaque(0, 0, 0))
            .with_cap(LineCap::Round)
            .with_join(LineJoin::Bevel)
            .with_miter_limit(10.0)
            .with_dash(DashPattern::new(vec![2.0, 1.0]).with_offset(0.5))
            .with_paint(Paint::Solid(Rgba::opaque(128, 128, 128)));
        assert_eq!(s.cap, LineCap::Round);
        assert_eq!(s.join, LineJoin::Bevel);
        assert_eq!(s.miter_limit, 10.0);
        let d = s.dash.expect("dash set");
        assert_eq!(d.array, vec![2.0, 1.0]);
        assert_eq!(d.offset, 0.5);
        match s.paint {
            Paint::Solid(c) => assert_eq!(c, Rgba::opaque(128, 128, 128)),
            _ => panic!("expected Paint::Solid"),
        }
    }

    #[test]
    fn dash_pattern_new_zero_offset() {
        let d = DashPattern::new(vec![1.0, 2.0, 3.0]);
        assert_eq!(d.array, vec![1.0, 2.0, 3.0]);
        assert_eq!(d.offset, 0.0);
    }

    #[test]
    fn dash_pattern_with_offset_sets_phase() {
        let d = DashPattern::new(vec![1.0]).with_offset(0.25);
        assert_eq!(d.offset, 0.25);
    }
}
