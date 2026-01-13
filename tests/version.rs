use std::process::Command;

fn find_executable() -> Option<String> {
    // Try a couple of common env-var names cargo may set for the built binary.
    // Different toolchains/platforms may mangle the binary name differently (hyphen vs underscore),
    // so check both first.
    if let Ok(v) = std::env::var("CARGO_BIN_EXE_jorik_cli") {
        return Some(v);
    }
    if let Ok(v) = std::env::var("CARGO_BIN_EXE_jorik-cli") {
        return Some(v);
    }

    // As a fallback, look for a likely built binary in target/debug/ (handles local runs)
    let candidates = [
        "target/debug/jorik-cli",
        "target/debug/jorik-cli.exe",
        "target/debug/jorik_cli",
        "target/debug/jorik_cli.exe",
    ];
    for cand in &candidates {
        if std::path::Path::new(cand).exists() {
            return Some(cand.to_string());
        }
    }

    // Not found; return None so the test can skip gracefully.
    None
}

#[test]
fn version_flag_shows_detection_info() {
    // find_executable may return None when cargo does not set CARGO_BIN_EXE_* env vars
    // (depends on how tests are invoked). Skip the test gracefully in that case.
    let exe = match find_executable() {
        Some(e) => e,
        None => {
            eprintln!(
                "Skipping integration test: built binary not found (CARGO_BIN_EXE_* not set and no target/debug binary present)."
            );
            return;
        }
    };
    let out = Command::new(exe)
        .args(&["--version", "--protocols"])
        .output()
        .expect("failed to execute binary");

    assert!(
        out.status.success(),
        "binary returned non-success; stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let s = String::from_utf8_lossy(&out.stdout);
    // Must include package version
    assert!(
        s.contains(env!("CARGO_PKG_VERSION")),
        "version string not found in output: {}",
        s
    );

    // Must include mention of at least one of the supported protocols (case-insensitive)
    let lower = s.to_lowercase();
    assert!(
        lower.contains("sixel") || lower.contains("iterm2") || lower.contains("kitty"),
        "protocol detection info not present in output: {}",
        s
    );
}

#[test]
fn version_flag_without_protocols_hides_detection_info() {
    // find_executable may return None when cargo does not set CARGO_BIN_EXE_* env vars
    // (depends on how tests are invoked). Skip the test gracefully in that case.
    let exe = match find_executable() {
        Some(e) => e,
        None => {
            eprintln!(
                "Skipping integration test: built binary not found (CARGO_BIN_EXE_* not set and no target/debug binary present)."
            );
            return;
        }
    };
    let out = Command::new(exe)
        .arg("--version")
        .output()
        .expect("failed to execute binary");

    assert!(
        out.status.success(),
        "binary returned non-success; stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let s = String::from_utf8_lossy(&out.stdout);
    // Must include package version
    assert!(
        s.contains(env!("CARGO_PKG_VERSION")),
        "version string not found in output: {}",
        s
    );

    // Protocol detection info must NOT be present in output when --protocols isn't supplied
    let lower = s.to_lowercase();
    assert!(
        !lower.contains("sixel") && !lower.contains("iterm2") && !lower.contains("kitty"),
        "protocol detection was unexpectedly present in output: {}",
        s
    );
}
