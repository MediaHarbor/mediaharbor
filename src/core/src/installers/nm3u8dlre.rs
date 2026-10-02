use super::{ArchiveKind, BinarySpec, SandboxPolicy, Target};

pub(crate) fn detect() -> Option<&'static str> {
    crate::venv_manager::find_managed_or_path("N_m3u8DL-RE").map(|_| "N_m3u8DL-RE")
}

pub(crate) const SPEC: BinarySpec = BinarySpec {
    id: "nm3u8dlre",
    display_name: "N_m3u8DL-RE",
    targets: &[
        Target {
            os: "linux",
            arch: "x86_64",
            urls: &["https://github.com/nilaoda/N_m3u8DL-RE/releases/download/v0.6.0-beta/N_m3u8DL-RE_v0.6.0-beta_linux-x64_20260629.tar.gz"],
            archive: ArchiveKind::TarGz,
            binary_names: &["N_m3u8DL-RE"],
        },
        Target {
            os: "linux",
            arch: "aarch64",
            urls: &["https://github.com/nilaoda/N_m3u8DL-RE/releases/download/v0.6.0-beta/N_m3u8DL-RE_v0.6.0-beta_linux-arm64_20260629.tar.gz"],
            archive: ArchiveKind::TarGz,
            binary_names: &["N_m3u8DL-RE"],
        },
        Target {
            os: "macos",
            arch: "x86_64",
            urls: &["https://github.com/nilaoda/N_m3u8DL-RE/releases/download/v0.6.0-beta/N_m3u8DL-RE_v0.6.0-beta_osx-x64_20260629.tar.gz"],
            archive: ArchiveKind::TarGz,
            binary_names: &["N_m3u8DL-RE"],
        },
        Target {
            os: "macos",
            arch: "aarch64",
            urls: &["https://github.com/nilaoda/N_m3u8DL-RE/releases/download/v0.6.0-beta/N_m3u8DL-RE_v0.6.0-beta_osx-arm64_20260629.tar.gz"],
            archive: ArchiveKind::TarGz,
            binary_names: &["N_m3u8DL-RE"],
        },
        Target {
            os: "windows",
            arch: "x86_64",
            urls: &["https://github.com/nilaoda/N_m3u8DL-RE/releases/download/v0.6.0-beta/N_m3u8DL-RE_v0.6.0-beta_win-x64_20260629.zip"],
            archive: ArchiveKind::Zip,
            binary_names: &["N_m3u8DL-RE.exe"],
        },
        Target {
            os: "windows",
            arch: "aarch64",
            urls: &["https://github.com/nilaoda/N_m3u8DL-RE/releases/download/v0.6.0-beta/N_m3u8DL-RE_v0.6.0-beta_win-arm64_20260629.zip"],
            archive: ArchiveKind::Zip,
            binary_names: &["N_m3u8DL-RE.exe"],
        },
    ],
    min_bytes: 1_000_000,
    sandbox: SandboxPolicy::Attempt,
    detect,
    install_hint:
        "Please download it manually from https://github.com/nilaoda/N_m3u8DL-RE/releases instead.",
    unsupported_hint:
        "Download it from https://github.com/nilaoda/N_m3u8DL-RE/releases and add it to your PATH.",
};

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "https://github.com/nilaoda/N_m3u8DL-RE/releases/download/v0.6.0-beta";

    /// The regression this table was extended for: macOS had no row, so the
    /// install button failed with "no managed build for macos-…".
    #[test]
    fn every_desktop_platform_has_a_target() {
        for (os, arch) in [
            ("linux", "x86_64"),
            ("linux", "aarch64"),
            ("macos", "x86_64"),
            ("macos", "aarch64"),
            ("windows", "x86_64"),
            ("windows", "aarch64"),
        ] {
            assert!(
                SPEC.targets.iter().any(|t| t.os == os && t.arch == arch),
                "{os}-{arch}"
            );
        }
    }

    #[test]
    fn every_target_points_at_the_pinned_release_and_names_one_binary() {
        for t in SPEC.targets {
            assert_eq!(t.urls.len(), 1, "{}-{}", t.os, t.arch);
            assert!(t.urls[0].starts_with(BASE), "{}-{}", t.os, t.arch);
            let expected = if t.os == "windows" {
                "N_m3u8DL-RE.exe"
            } else {
                "N_m3u8DL-RE"
            };
            assert_eq!(t.binary_names, &[expected], "{}-{}", t.os, t.arch);
        }
    }
}
