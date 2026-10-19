//! The updater end to end against a local server: reading the manifest, refusing a download
//! the key did not sign, and (macOS) swapping the app bundle.
//!
//! `fixtures/update` holds a tiny "Kiln.app" archive signed with a throwaway test key; only its
//! public half is kept. To remake it:
//! `cargo packager signer generate --path /tmp/test.key --ci`, tar a `Kiln.app` folder, then
//! `cargo packager signer sign -k /tmp/test.key --password "" Kiln.app.tar.gz`.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::Arc;

use kiln::update::{self, Source};
use semver::Version;

const ARCHIVE: &[u8] = include_bytes!("fixtures/update/Kiln.app.tar.gz");
const SIGNATURE: &str = include_str!("fixtures/update/Kiln.app.tar.gz.sig");
const PUBKEY: &str = include_str!("fixtures/update/test.key.pub");

/// Serves `/latest.json`, `/good.tar.gz` and `/tampered.tar.gz` until the test ends.
fn serve(version: &str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let target = cargo_packager_updater::target().unwrap();
    let manifest = |file: &str| {
        format!(
            r#"{{"version":"{version}","notes":"Test release","pub_date":"2026-10-19T00:00:00Z",
               "platforms":{{"{target}":{{"url":"{base}/{file}","signature":"{}","format":"app"}}}}}}"#,
            SIGNATURE.trim()
        )
    };
    let good = Arc::new(manifest("good.tar.gz"));
    let bad = Arc::new(manifest("tampered.tar.gz"));
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut request = String::new();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            if reader.read_line(&mut request).is_err() {
                continue;
            }
            // Drain the headers.
            let mut line = String::new();
            while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
                line.clear();
            }
            let path = request.split(' ').nth(1).unwrap_or("/").to_string();
            let (kind, body): (&str, Vec<u8>) = match path.as_str() {
                "/latest.json" => ("application/json", good.as_bytes().to_vec()),
                "/tampered.json" => ("application/json", bad.as_bytes().to_vec()),
                "/good.tar.gz" => ("application/octet-stream", ARCHIVE.to_vec()),
                "/tampered.tar.gz" => {
                    let mut bytes = ARCHIVE.to_vec();
                    let middle = bytes.len() / 2;
                    bytes[middle] ^= 0xff;
                    ("application/octet-stream", bytes)
                }
                _ => ("text/plain", b"not found".to_vec()),
            };
            let status = if kind == "text/plain" {
                "404 Not Found"
            } else {
                "200 OK"
            };
            let _ = write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(&body);
        }
    });
    base
}

fn fake_app(dir: &Path) -> std::path::PathBuf {
    let app = dir.join("Kiln.app");
    std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
    std::fs::write(app.join("Contents/MacOS/kiln"), "old build\n").unwrap();
    app
}

#[test]
fn finds_a_newer_release_and_ignores_older_ones() {
    let base = serve("2026.10.26");
    let source = Source::new(&format!("{base}/latest.json"), PUBKEY).unwrap();
    let dir = tempfile::tempdir().unwrap();

    let current = Version::parse("2026.10.19").unwrap();
    let update = update::check(&source, &current, fake_app(dir.path()))
        .unwrap()
        .expect("a newer version is on offer");
    assert_eq!(update.version, "2026.10.26");
    assert_eq!(update.body.as_deref(), Some("Test release"));

    let newer = Version::parse("2026.11.2").unwrap();
    assert!(
        update::check(&source, &newer, fake_app(dir.path()))
            .unwrap()
            .is_none()
    );
}

#[test]
fn refuses_a_download_the_key_did_not_sign() {
    let base = serve("99.0.0");
    let source = Source::new(&format!("{base}/tampered.json"), PUBKEY).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let app = fake_app(dir.path());
    let update = update::check(&source, &Version::new(0, 0, 1), app.clone())
        .unwrap()
        .unwrap();
    let err = update::download_and_install(&update).unwrap_err();
    assert!(format!("{err:#}").contains("download"), "{err:#}");
    // Nothing was touched.
    assert_eq!(
        std::fs::read_to_string(app.join("Contents/MacOS/kiln")).unwrap(),
        "old build\n"
    );
}

#[test]
#[cfg(target_os = "macos")]
fn installs_a_signed_update_over_the_app_bundle() {
    let base = serve("99.0.0");
    let source = Source::new(&format!("{base}/latest.json"), PUBKEY).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let app = fake_app(dir.path());
    let update = update::check(&source, &Version::new(0, 0, 1), app.clone())
        .unwrap()
        .unwrap();
    update::download_and_install(&update).unwrap();
    assert_eq!(
        std::fs::read_to_string(app.join("Contents/MacOS/kiln")).unwrap(),
        "new build\n"
    );
}

#[test]
fn reads_the_manifest_the_release_script_writes() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Kiln.app.tar.gz"), ARCHIVE).unwrap();
    std::fs::write(dir.path().join("Kiln.app.tar.gz.sig"), SIGNATURE).unwrap();
    let script = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../scripts/update-manifest.py"
    );
    let python = if cfg!(windows) { "python" } else { "python3" };
    let Ok(output) = std::process::Command::new(python)
        .args([script, "2026.10.26", "https://example.com/kiln"])
        .arg(dir.path())
        .output()
    else {
        eprintln!("no python here; skipped");
        return;
    };
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let release: cargo_packager_updater::RemoteRelease =
        serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(release.version, Version::parse("2026.10.26").unwrap());
    assert_eq!(
        release.download_url("macos-aarch64").unwrap().as_str(),
        "https://example.com/kiln/Kiln.app.tar.gz"
    );
    assert_eq!(
        release.signature("macos-aarch64").unwrap(),
        SIGNATURE.trim()
    );
}
