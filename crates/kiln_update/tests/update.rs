//! The updater end to end against a local server: reading the manifest, refusing a download
//! the key did not sign, swapping the app bundle (macOS) or the AppImage (Linux), and reading
//! the manifest the release script writes.
//!
//! `fixtures/update` holds a tiny "Kiln.app" archive and a stand-in "Kiln.AppImage", signed with
//! a throwaway test key; only its public half is kept. To remake them:
//! `cargo packager signer generate --path /tmp/test.key --ci`, then
//! `cargo packager signer sign -k /tmp/test.key --password "" <file>`.
//!
//! `accepts_a_signed_release_build` is the same check on a real build: the release workflow
//! runs it (`--ignored`) on the package it has just made and signed.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};

use kiln_update::{Source, UpdateFormat, Version};

const ARCHIVE: &[u8] = include_bytes!("fixtures/update/Kiln.app.tar.gz");
const ARCHIVE_SIG: &str = include_str!("fixtures/update/Kiln.app.tar.gz.sig");
const APPIMAGE: &[u8] = include_bytes!("fixtures/update/Kiln.AppImage");
const APPIMAGE_SIG: &str = include_str!("fixtures/update/Kiln.AppImage.sig");
const PUBKEY: &str = include_str!("fixtures/update/test.key.pub");

/// Serves `files` (path, bytes) over HTTP until the test ends; returns the base address.
fn serve(files: Vec<(String, Vec<u8>)>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut request = String::new();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            if reader.read_line(&mut request).is_err() {
                continue;
            }
            let mut line = String::new();
            while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
                line.clear();
            }
            let path = request.split(' ').nth(1).unwrap_or("/").to_string();
            let found = files.iter().find(|(p, _)| *p == path);
            let (status, body) = match found {
                Some((_, body)) => ("200 OK", body.clone()),
                None => ("404 Not Found", b"not found".to_vec()),
            };
            let _ = write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(&body);
        }
    });
    base
}

/// A manifest offering `version` for this machine, downloadable from `url`.
fn manifest(version: &str, url: &str, signature: &str, format: &str) -> Vec<u8> {
    let platform = kiln_update::platform().unwrap();
    format!(
        r#"{{"version":"{version}","notes":"Test release","pub_date":"2026-10-19T00:00:00Z",
           "platforms":{{"{platform}":{{"url":"{url}","signature":"{}","format":"{format}"}}}}}}"#,
        signature.trim()
    )
    .into_bytes()
}

/// Serves a release of `package` (with `signature`) at `/latest.json`.
fn release(version: &str, package: Vec<u8>, signature: &str, format: &str) -> Source {
    // The package's address is only known once its server runs, so the manifest gets its own.
    let files = serve(vec![("/package".into(), package)]);
    let url = format!("{files}/package");
    let base = serve(vec![(
        "/latest.json".into(),
        manifest(version, &url, signature, format),
    )]);
    Source::new(&format!("{base}/latest.json"), PUBKEY).unwrap()
}

fn fake_app(dir: &Path) -> PathBuf {
    let app = dir.join("Kiln.app");
    std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
    std::fs::write(app.join("Contents/MacOS/kiln"), "old build\n").unwrap();
    app
}

#[test]
fn finds_a_newer_release_and_ignores_older_ones() {
    let source = release("2026.10.26", ARCHIVE.to_vec(), ARCHIVE_SIG, "app");
    let dir = tempfile::tempdir().unwrap();
    let current = Version::parse("2026.10.19").unwrap();
    let update = kiln_update::check(&source, &current, fake_app(dir.path()))
        .unwrap()
        .expect("a newer version is on offer");
    assert_eq!(update.version, "2026.10.26");
    assert_eq!(update.body.as_deref(), Some("Test release"));

    let newer = Version::parse("2026.11.2").unwrap();
    assert!(
        kiln_update::check(&source, &newer, fake_app(dir.path()))
            .unwrap()
            .is_none()
    );
}

#[test]
fn refuses_a_download_the_key_did_not_sign() {
    let mut tampered = ARCHIVE.to_vec();
    let middle = tampered.len() / 2;
    tampered[middle] ^= 0xff;
    let source = release("99.0.0", tampered, ARCHIVE_SIG, "app");
    let dir = tempfile::tempdir().unwrap();
    let app = fake_app(dir.path());
    let update = kiln_update::check(&source, &Version::new(0, 0, 1), app.clone())
        .unwrap()
        .unwrap();
    let err = kiln_update::download_and_install(&update).unwrap_err();
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
    let source = release("99.0.0", ARCHIVE.to_vec(), ARCHIVE_SIG, "app");
    let dir = tempfile::tempdir().unwrap();
    let app = fake_app(dir.path());
    let update = kiln_update::check(&source, &Version::new(0, 0, 1), app.clone())
        .unwrap()
        .unwrap();
    kiln_update::download_and_install(&update).unwrap();
    assert_eq!(
        std::fs::read_to_string(app.join("Contents/MacOS/kiln")).unwrap(),
        "new build\n"
    );
}

#[test]
#[cfg(target_os = "linux")]
fn an_appimage_replaces_itself_with_a_signed_update() {
    use std::os::unix::fs::PermissionsExt;
    let source = release("99.0.0", APPIMAGE.to_vec(), APPIMAGE_SIG, "appimage");
    let dir = tempfile::tempdir().unwrap();
    let installed = dir.path().join("Kiln.AppImage");
    std::fs::write(&installed, "old appimage build\n").unwrap();
    std::fs::set_permissions(&installed, std::fs::Permissions::from_mode(0o755)).unwrap();
    let update = kiln_update::check(&source, &Version::new(0, 0, 1), installed.clone())
        .unwrap()
        .unwrap();
    assert!(matches!(update.format, UpdateFormat::AppImage));
    kiln_update::download_and_install(&update).unwrap();
    assert_eq!(
        std::fs::read_to_string(&installed).unwrap(),
        "new appimage build\n"
    );
    let mode = std::fs::metadata(&installed).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o755, "the AppImage stays executable");
}

#[test]
fn reads_the_manifest_the_release_script_writes() {
    let dir = tempfile::tempdir().unwrap();
    let write = |name: &str, bytes: &[u8]| std::fs::write(dir.path().join(name), bytes).unwrap();
    write("Kiln.app.tar.gz", ARCHIVE);
    write("Kiln.app.tar.gz.sig", ARCHIVE_SIG.as_bytes());
    write("Kiln-2026.10.26-x86_64.AppImage", APPIMAGE);
    write(
        "Kiln-2026.10.26-x86_64.AppImage.sig",
        APPIMAGE_SIG.as_bytes(),
    );
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
        ARCHIVE_SIG.trim()
    );
    assert_eq!(
        release.download_url("linux-x86_64").unwrap().as_str(),
        "https://example.com/kiln/Kiln-2026.10.26-x86_64.AppImage"
    );
    assert!(matches!(
        release.format("linux-x86_64").unwrap(),
        UpdateFormat::AppImage
    ));
}

/// The real thing: the release workflow runs this on the package it has just built, signed
/// with the release key. A local server offers it as a newer version; the updater must accept
/// the signature and install it over a stand-in installed copy.
///
/// `KILN_E2E_PACKAGE`: the AppImage or `Kiln.app.tar.gz`, with its `.sig` next to it.
/// `KILN_E2E_PUBKEY`: the release's public key.
#[test]
#[ignore = "needs a signed release build (KILN_E2E_PACKAGE, KILN_E2E_PUBKEY)"]
fn accepts_a_signed_release_build() {
    let package = PathBuf::from(std::env::var("KILN_E2E_PACKAGE").expect("KILN_E2E_PACKAGE"));
    let pubkey = std::env::var("KILN_E2E_PUBKEY").expect("KILN_E2E_PUBKEY");
    let bytes = std::fs::read(&package).unwrap();
    let signature = std::fs::read_to_string(format!("{}.sig", package.display())).unwrap();
    let format = kiln_update::format_for(&package).expect("an update file");
    let name = match format {
        UpdateFormat::App => "app",
        UpdateFormat::AppImage => "appimage",
        _ => panic!("only app bundles and AppImages are installed in place"),
    };

    let files = serve(vec![("/package".into(), bytes.clone())]);
    let url = format!("{files}/package");
    let base = serve(vec![(
        "/latest.json".into(),
        manifest("99999.0.0", &url, &signature, name),
    )]);
    let source = Source::new(&format!("{base}/latest.json"), &pubkey).unwrap();

    let dir = tempfile::tempdir().unwrap();
    let installed = match format {
        UpdateFormat::App => fake_app(dir.path()),
        _ => {
            let path = dir.path().join("Kiln.AppImage");
            std::fs::write(&path, "old appimage build\n").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
            path
        }
    };
    let current = Version::parse(env!("CARGO_PKG_VERSION")).unwrap();
    let update = kiln_update::check(&source, &current, installed.clone())
        .unwrap()
        .expect("the build is offered");
    kiln_update::download_and_install(&update).expect("the signed build is accepted");
    match format {
        UpdateFormat::App => assert!(installed.join("Contents/MacOS/kiln").is_file()),
        _ => assert_eq!(std::fs::read(&installed).unwrap(), bytes),
    }
    println!(
        "accepted and installed {} ({} bytes)",
        package.display(),
        bytes.len()
    );
}

/// A release's `latest.json`, checked the way the app would: every platform listed in
/// `KILN_E2E_PLATFORMS` is there, and each file's signature verifies against the release key.
/// The release workflow runs it (`--ignored`) on the manifest it writes, before publishing.
///
/// `KILN_E2E_MANIFEST`: the manifest. `KILN_E2E_DIR`: the folder with the release files.
/// `KILN_E2E_PUBKEY`: the release's public key. `KILN_E2E_PLATFORMS`: comma-separated keys.
#[test]
#[ignore = "needs a release's files (KILN_E2E_MANIFEST, KILN_E2E_DIR, KILN_E2E_PUBKEY)"]
fn release_manifest_verifies() {
    use base64::Engine as _;
    let var = |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("{name}"));
    // Relative paths are from the repository root (cargo runs tests from the crate's folder).
    let path = |name: &str| {
        let path = PathBuf::from(var(name));
        if path.is_relative() {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join(path)
        } else {
            path
        }
    };
    let manifest = std::fs::read(path("KILN_E2E_MANIFEST")).expect("the manifest");
    let dir = path("KILN_E2E_DIR");
    let decode = |text: &str| {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(text.trim())
            .unwrap();
        String::from_utf8(bytes).unwrap()
    };
    let key = minisign_verify::PublicKey::decode(&decode(&var("KILN_E2E_PUBKEY"))).unwrap();

    let release: cargo_packager_updater::RemoteRelease =
        serde_json::from_slice(&manifest).expect("a manifest the app can read");
    for platform in var("KILN_E2E_PLATFORMS").split(',') {
        let url = release
            .download_url(platform)
            .unwrap_or_else(|_| panic!("{platform} is missing from the manifest"));
        let name = url.path_segments().unwrap().next_back().unwrap();
        let name = percent_decode(name);
        let data = std::fs::read(dir.join(&name)).unwrap_or_else(|_| panic!("{name} is missing"));
        let signature =
            minisign_verify::Signature::decode(&decode(release.signature(platform).unwrap()))
                .unwrap();
        key.verify(&data, &signature, true)
            .unwrap_or_else(|err| panic!("{platform}: {name} does not verify: {err}"));
        println!("{platform}: {name} ({} bytes) verifies", data.len());
    }
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(byte) = u8::from_str_radix(&text[i + 1..i + 3], 16)
        {
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap()
}
