//! The launch scripts are shell; nothing else watches them. These tests
//! keep `proton/` honest without needing a prefix or a Proton install.

#[cfg(all(test, unix))]
mod tests {
    use std::path::{Path, PathBuf};
    use std::process::Command;

    fn root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("xtask lives in the workspace")
            .into()
    }

    #[test]
    fn proton_scripts_parse_and_are_executable() {
        let root = root();
        for script in ["proton/run.sh", "proton/smoke.sh"] {
            let status = Command::new("bash")
                .args(["-n", script])
                .current_dir(&root)
                .status()
                .expect("bash");
            assert!(status.success(), "{script} does not parse");
            let mode = std::fs::metadata(root.join(script))
                .expect("script exists")
                .permissions();
            use std::os::unix::fs::PermissionsExt;
            assert_ne!(mode.mode() & 0o111, 0, "{script} is not executable");
        }
    }

    #[test]
    fn proton_launcher_help_needs_nothing_installed() {
        let out = Command::new("bash")
            .args(["proton/run.sh", "--help"])
            .current_dir(root())
            .output()
            .expect("bash");
        assert!(out.status.success(), "run.sh --help failed");
        let text = String::from_utf8_lossy(&out.stdout);
        for flag in ["--prefix", "--exe", "--proton", "--z", "--config"] {
            assert!(text.contains(flag), "usage misses {flag}");
        }
    }
}
