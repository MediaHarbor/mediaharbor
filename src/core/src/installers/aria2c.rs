use super::{ArchiveKind, BinarySpec, SandboxPolicy, Target};

/// aria2 publishes no macOS binary and neither does the static-build mirror, so
/// the error a Mac user gets should name the one command that actually works
/// rather than a vague "use your package manager".
#[cfg(target_os = "macos")]
const UNSUPPORTED_HINT: &str = "Install it with Homebrew instead: brew install aria2";
#[cfg(not(target_os = "macos"))]
const UNSUPPORTED_HINT: &str = "Install aria2 via your system package manager.";

pub(crate) fn detect() -> Option<&'static str> {
    crate::venv_manager::find_managed_or_path("aria2c").map(|_| "aria2c")
}

pub(crate) const SPEC: BinarySpec = BinarySpec {
    id: "aria2c",
    display_name: "aria2c",
    // The abcfy2 mirror publishes `.zip`, never `.tar.gz` — the old URLs here
    // asked for tarballs that have never existed and 404'd on every Linux arch.
    // Asset names carry no version, so `releases/latest` stays valid.
    targets: &[
        Target {
            os: "linux",
            arch: "x86_64",
            urls: &["https://github.com/abcfy2/aria2-static-build/releases/latest/download/aria2-x86_64-linux-musl_static.zip"],
            archive: ArchiveKind::Zip,
            binary_names: &["aria2c"],
        },
        Target {
            os: "linux",
            arch: "aarch64",
            urls: &["https://github.com/abcfy2/aria2-static-build/releases/latest/download/aria2-aarch64-linux-musl_static.zip"],
            archive: ArchiveKind::Zip,
            binary_names: &["aria2c"],
        },
        Target {
            os: "linux",
            arch: "arm",
            // `arm` spans armv7 hard-float and the older soft-float ABI; try hf
            // first. (The previous `arm-linux-musleabihf` asset name is not one
            // the mirror has ever published under any extension.)
            urls: &[
                "https://github.com/abcfy2/aria2-static-build/releases/latest/download/aria2-armv7-linux-musleabihf_static.zip",
                "https://github.com/abcfy2/aria2-static-build/releases/latest/download/aria2-arm-linux-musleabi_static.zip",
            ],
            archive: ArchiveKind::Zip,
            binary_names: &["aria2c"],
        },
        Target {
            os: "windows",
            arch: "x86_64",
            urls: &["https://github.com/aria2/aria2/releases/download/release-1.37.0/aria2-1.37.0-win-64bit-build1.zip"],
            archive: ArchiveKind::Zip,
            binary_names: &["aria2c.exe"],
        },
    ],
    min_bytes: 500_000,
    sandbox: SandboxPolicy::Attempt,
    detect,
    install_hint: "Please install aria2 via your system package manager instead.",
    unsupported_hint: UNSUPPORTED_HINT,
};

#[cfg(test)]
mod tests {
    use super::*;

    fn target_for(os: &str, arch: &str) -> Option<&'static Target> {
        SPEC.targets.iter().find(|t| t.os == os && t.arch == arch)
    }

    /// aria2 ships no macOS build, and aria2-next — the maintained fork that
    /// does — rejects `--always-resume`, which yt-dlp passes on every download
    /// and documents as unchangeable. So Mac users are pointed at Homebrew and
    /// the hint has to name the command.
    #[test]
    fn macos_is_absent_and_its_hint_names_homebrew() {
        assert!(target_for("macos", "aarch64").is_none());
        assert!(target_for("macos", "x86_64").is_none());
        #[cfg(target_os = "macos")]
        assert!(SPEC.unsupported_hint.contains("brew install aria2"));
    }

    /// The regression: every Linux row asked for a `.tar.gz` the mirror does not
    /// publish, so all three 404'd.
    #[test]
    fn every_linux_target_downloads_a_zip_that_exists() {
        for t in SPEC.targets.iter().filter(|t| t.os == "linux") {
            assert!(matches!(t.archive, ArchiveKind::Zip), "{}", t.arch);
            for url in t.urls {
                assert!(url.ends_with("_static.zip"), "{url}");
                assert!(!url.contains("musleabihf_static.tar"), "{url}");
            }
        }
    }

    #[test]
    fn every_target_yields_one_binary_named_aria2c() {
        for t in SPEC.targets {
            assert!(!t.urls.is_empty(), "{}-{}", t.os, t.arch);
            let expected = if t.os == "windows" {
                "aria2c.exe"
            } else {
                "aria2c"
            };
            assert_eq!(t.binary_names, &[expected], "{}-{}", t.os, t.arch);
        }
    }
}
