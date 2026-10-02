//! Per-codec hardware engine probing.
//!
//! Each HW-accel sibling crate (oxideav-nvidia, oxideav-vaapi,
//! oxideav-vdpau, oxideav-vulkan-video, oxideav-videotoolbox) attaches
//! an [`EngineProbeFn`] to every [`crate::CodecInfo`] it registers via
//! [`crate::CodecInfo::with_engine_probe`]. The CLI's `info` command
//! (and any other consumer) calls the probe on demand to enumerate
//! the physical / logical engines that backend can dispatch to —
//! GPU name, driver version, per-codec capability matrix, etc.
//!
//! Probes are called on demand, not at registration time, so the cost
//! of opening device handles + querying capabilities is only paid
//! when someone asks. Probes should be idempotent and side-effect
//! free; consumers may call them more than once per process.
//!
//! There is no distributed slice and no collection macro: engine info
//! travels with each [`crate::CodecInfo`], matching the explicit-calls
//! pattern already used by `oxideav-meta`'s `register_all`. Consumers
//! that want to enumerate engines walk the codec registry, group
//! entries by [`crate::CodecInfo::engine_id`], and call each backend's
//! [`EngineProbeFn`] at most once per group.

/// A single hardware engine the backend can dispatch to. For NVIDIA /
/// Vulkan / VA-API DRM, this is one entry per physical GPU. For
/// VDPAU on a single-X11-display system, this is one entry per X
/// screen. For VideoToolbox on Apple Silicon, this is one entry per
/// SoC.
#[derive(Clone, Debug)]
pub struct HwDeviceInfo {
    /// Human-readable device name. e.g. "NVIDIA GeForce RTX 5080",
    /// "Intel(R) UHD Graphics 770", "Apple M3 Max Media Engine".
    pub name: String,
    /// Driver / runtime version, if reportable. e.g. "580.95.05",
    /// "Mesa 24.2", "VideoToolbox (system)".
    pub driver_version: Option<String>,
    /// API version the backend speaks. e.g. "CUDA 12.6",
    /// "VDPAU API 1", "Vulkan 1.4", "VA-API 1.22".
    pub api_version: Option<String>,
    /// On-card memory in bytes if known. Discrete GPUs report a real
    /// figure; integrated and shared-memory engines usually report
    /// `None`.
    pub total_memory_bytes: Option<u64>,
    /// Backend-specific extras keyed by string. e.g. for NVIDIA:
    /// `("compute_capability", "12.0")`. CLI prints these as
    /// `key = value` in a sub-block. Order is preserved.
    pub extra: Vec<(String, String)>,
    /// Per-codec capabilities for codecs this engine can decode and/or
    /// encode.
    pub codecs: Vec<HwCodecCaps>,
}

/// Capabilities of a single codec on a single device.
#[derive(Clone, Debug)]
pub struct HwCodecCaps {
    /// Codec id matching `oxideav_core::CodecId`. e.g. "h264", "hevc",
    /// "av1", "vp9". Should be the same string the SW codec uses.
    pub codec: String,
    /// Whether this device can decode this codec.
    pub decode: bool,
    /// Whether this device can encode this codec.
    pub encode: bool,
    /// Max coded width supported, if reportable.
    pub max_width: Option<u32>,
    /// Max coded height supported, if reportable.
    pub max_height: Option<u32>,
    /// Max bit-depth supported, if reportable. Typically 8, 10, or 12.
    pub max_bit_depth: Option<u32>,
    /// Profile names (backend-specific). e.g. for H.264 on NVDEC:
    /// `["Baseline", "Main", "High"]`.
    pub profiles: Vec<String>,
    /// Backend-specific extras (e.g. `("max_dpb_slots", "17")`).
    pub extra: Vec<(String, String)>,
}

/// Function signature for a backend's engine probe. Returns one entry
/// per device the backend currently sees. Call cheaply; consumers may
/// call multiple times per process.
pub type EngineProbeFn = fn() -> Vec<HwDeviceInfo>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hw_device_info_clone_and_extras_round_trip() {
        let info = HwDeviceInfo {
            name: "Test GPU".into(),
            driver_version: Some("1.0".into()),
            api_version: Some("API 1".into()),
            total_memory_bytes: Some(16 * 1024 * 1024 * 1024),
            extra: vec![("compute_capability".into(), "12.0".into())],
            codecs: vec![HwCodecCaps {
                codec: "h264".into(),
                decode: true,
                encode: true,
                max_width: Some(8192),
                max_height: Some(8192),
                max_bit_depth: Some(8),
                profiles: vec!["Baseline".into(), "Main".into(), "High".into()],
                extra: vec![],
            }],
        };
        let clone = info.clone();
        assert_eq!(clone.name, "Test GPU");
        assert_eq!(clone.driver_version.as_deref(), Some("1.0"));
        assert_eq!(clone.api_version.as_deref(), Some("API 1"));
        assert_eq!(clone.total_memory_bytes, Some(16 * 1024 * 1024 * 1024));
        assert_eq!(clone.extra.len(), 1);
        assert_eq!(clone.extra[0].0, "compute_capability");
        assert_eq!(clone.extra[0].1, "12.0");
        assert_eq!(clone.codecs.len(), 1);
        assert_eq!(clone.codecs[0].codec, "h264");
        assert!(clone.codecs[0].decode);
        assert!(clone.codecs[0].encode);
        assert_eq!(clone.codecs[0].profiles.len(), 3);
    }

    #[test]
    fn engine_probe_fn_is_callable() {
        fn empty_probe() -> Vec<HwDeviceInfo> {
            vec![]
        }
        let probe: EngineProbeFn = empty_probe;
        assert!(probe().is_empty());
    }
}
