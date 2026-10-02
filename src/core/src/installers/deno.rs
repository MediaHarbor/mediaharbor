use super::{ArchiveKind, BinarySpec, SandboxPolicy, Target};

pub(crate) fn detect() -> Option<&'static str> {
    ["deno", "bun", "node"]
        .into_iter()
        .find(|runtime| crate::venv_manager::find_managed_or_path(runtime).is_some())
}

pub(crate) const SPEC: BinarySpec = BinarySpec {
    id: "deno",
    display_name: "Deno",
    targets: &[
        Target {
            os: "linux",
            arch: "x86_64",
            urls: &["https://github.com/denoland/deno/releases/latest/download/deno-x86_64-unknown-linux-gnu.zip"],
            archive: ArchiveKind::Zip,
            binary_names: &["deno"],
        },
        Target {
            os: "linux",
            arch: "aarch64",
            urls: &["https://github.com/denoland/deno/releases/latest/download/deno-aarch64-unknown-linux-gnu.zip"],
            archive: ArchiveKind::Zip,
            binary_names: &["deno"],
        },
        Target {
            os: "macos",
            arch: "x86_64",
            urls: &["https://github.com/denoland/deno/releases/latest/download/deno-x86_64-apple-darwin.zip"],
            archive: ArchiveKind::Zip,
            binary_names: &["deno"],
        },
        Target {
            os: "macos",
            arch: "aarch64",
            urls: &["https://github.com/denoland/deno/releases/latest/download/deno-aarch64-apple-darwin.zip"],
            archive: ArchiveKind::Zip,
            binary_names: &["deno"],
        },
        Target {
            os: "windows",
            arch: "x86_64",
            urls: &["https://github.com/denoland/deno/releases/latest/download/deno-x86_64-pc-windows-msvc.zip"],
            archive: ArchiveKind::Zip,
            binary_names: &["deno.exe"],
        },
    ],
    min_bytes: 1_000_000,
    sandbox: SandboxPolicy::Attempt,
    detect,
    install_hint: "Please install Deno from https://deno.com instead.",
    unsupported_hint: "Install a JavaScript runtime (deno, node, or bun) from https://deno.com.",
};
