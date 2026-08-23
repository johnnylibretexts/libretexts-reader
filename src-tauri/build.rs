use std::env;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};

const PDFIUM_RELEASE: &str = "chromium/7789";
const PDFIUM_RELEASE_URL_COMPONENT: &str = "chromium%2F7789";

// Bump when the *extraction* changes, not when the archive does.
//
// The `.sha256` markers used to record only archive_sha256, which identifies
// the bytes downloaded and says nothing about what was unpacked from them. A
// CI cache holding a tree extracted by older logic therefore satisfied the
// check and was reused verbatim -- which is exactly how the dropped-symlink
// fix below first landed with no observable effect: the gate kept reporting
// the same seven unresolvable libraries against a cached, stale extraction.
//
// Kept per asset so bumping one does not force the other to re-download.
const PDFIUM_EXTRACT_VERSION: u32 = 1;

/// Marker contents: which archive, and which extraction produced this tree.
fn marker_value(archive_sha256: &str, extract_version: u32) -> String {
    format!("{archive_sha256} extract{extract_version}")
}

struct PdfiumAsset {
    target: &'static str,
    asset_name: &'static str,
    archive_sha256: &'static str,
    library_path_in_archive: &'static str,
    library_file_name: &'static str,
}

const UPDATER_PUBKEY_PLACEHOLDER: &str = "TAURI_UPDATER_PUBKEY_PLACEHOLDER";

fn main() {
    check_updater_pubkey();

    mirror_licenses_into_bundle(&PathBuf::from(
        env::var("CARGO_MANIFEST_DIR").expect("manifest dir"),
    ));

    let pdfium_library = ensure_pdfium();
    println!(
        "cargo:rustc-env=PDFIUM_LIBRARY_PATH={}",
        pdfium_library.display()
    );

    tauri_build::build()
}

/// Guard against shipping a public build whose auto-updater is configured with
/// the placeholder signing key. A real release must replace the `pubkey` in
/// `tauri.conf.json` (see `RELEASE.md`). This always warns when the placeholder
/// is present, and hard-fails the build when `LIBRETEXTS_READER_REQUIRE_UPDATER_KEY`
/// is set (intended for release CI), so a misconfigured updater cannot ship.
fn check_updater_pubkey() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let config_path = manifest_dir.join("tauri.conf.json");
    println!("cargo:rerun-if-changed={}", config_path.display());
    println!("cargo:rerun-if-env-changed=LIBRETEXTS_READER_REQUIRE_UPDATER_KEY");

    let config = fs::read_to_string(&config_path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", config_path.display()));

    // Treat any set value as enabled unless explicitly "0"/"false" so the gate
    // fails closed rather than being accidentally disabled by a truthy value.
    let require_key = match env::var("LIBRETEXTS_READER_REQUIRE_UPDATER_KEY") {
        Ok(value) => {
            let value = value.trim();
            !value.is_empty() && value != "0" && !value.eq_ignore_ascii_case("false")
        }
        Err(_) => false,
    };

    // Inspect the actual configured key, not just the presence of the
    // placeholder string, so a missing/empty pubkey cannot bypass the gate.
    let config_json = serde_json::from_str::<serde_json::Value>(&config).ok();
    let updater = config_json.as_ref().and_then(|value| {
        value
            .get("plugins")
            .and_then(|plugins| plugins.get("updater"))
    });
    // No updater block -> nothing here to inspect, so this returns rather than
    // failing. That is deliberate, and it is also the limit of what this guard
    // can do: it sees the *configuration*, so it catches a block whose pubkey is
    // missing or still the placeholder, and cannot catch the plugin being added
    // as a dependency with no block written at all. Nothing in this file can --
    // build.rs has no view of the dependency graph.
    //
    // `scripts/ci/check-updater-key.sh` closes that case from the other side: it
    // fails when `tauri-plugin-updater` is a dependency without a real pubkey
    // configured, and runs in the shared gate both ci.yml and release.yml call.
    // Do not weaken this into failing on a missing block -- the updater is
    // deliberately absent in v0.1.0, so that would fail every build today.
    if updater.is_none() {
        return;
    }
    let pubkey = updater
        .and_then(|u| u.get("pubkey"))
        .and_then(|k| k.as_str());
    let pubkey_is_real =
        pubkey.is_some_and(|key| !key.trim().is_empty() && key != UPDATER_PUBKEY_PLACEHOLDER);

    if pubkey_is_real {
        return;
    }

    if require_key {
        panic!(
            "Updater pubkey is missing or still the placeholder \
             ({UPDATER_PUBKEY_PLACEHOLDER}). Generate a key with \
             `npm run tauri -- signer generate` and set \
             plugins.updater.pubkey before a release build. See RELEASE.md."
        );
    }

    println!(
        "cargo:warning=Updater pubkey is not release-ready (missing or placeholder); \
         auto-update will not verify signatures. See RELEASE.md. Set \
         LIBRETEXTS_READER_REQUIRE_UPDATER_KEY=1 to make this fatal for release builds."
    );
}

fn ensure_pdfium() -> PathBuf {
    let target = env::var("TARGET").expect("TARGET must be set by Cargo");
    let asset = pdfium_asset_for_target(&target);
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let resource_dir = manifest_dir
        .join("resources")
        .join("pdfium")
        .join(asset.target);
    let library_path = resource_dir.join(asset.library_file_name);
    let marker_path = resource_dir.join(".asset-sha256");

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={}", marker_path.display());

    let expected_marker = marker_value(asset.archive_sha256, PDFIUM_EXTRACT_VERSION);
    if library_path.exists()
        && fs::read_to_string(&marker_path).is_ok_and(|value| value.trim() == expected_marker)
    {
        return library_path;
    }

    fs::create_dir_all(&resource_dir).expect("create PDFium resource directory");

    let archive = download_pdfium(&asset);
    verify_sha256(&archive, asset.archive_sha256, asset.asset_name);
    extract_library(&archive, &asset, &library_path);
    extract_license_files(&archive, &manifest_dir);
    fs::write(marker_path, format!("{expected_marker}\n")).expect("write PDFium marker");

    library_path
}

fn pdfium_asset_for_target(target: &str) -> PdfiumAsset {
    match target {
        "aarch64-apple-darwin" => PdfiumAsset {
            target: "aarch64-apple-darwin",
            asset_name: "pdfium-mac-arm64.tgz",
            archive_sha256: "3110873c852db65a4e603423671db3fc455e4c70cf3a4895b53bc4141f74111b",
            library_path_in_archive: "lib/libpdfium.dylib",
            library_file_name: "libpdfium.dylib",
        },
        "x86_64-apple-darwin" => PdfiumAsset {
            target: "x86_64-apple-darwin",
            asset_name: "pdfium-mac-x64.tgz",
            archive_sha256: "b30cc25dff1bd0581d2d2a518ee7c350190b51fa21502a68aa5f6862e1e5423d",
            library_path_in_archive: "lib/libpdfium.dylib",
            library_file_name: "libpdfium.dylib",
        },
        "x86_64-pc-windows-msvc" => PdfiumAsset {
            target: "x86_64-pc-windows-msvc",
            asset_name: "pdfium-win-x64.tgz",
            archive_sha256: "5d93c5b5677bc38c5b13f5f2314fd4e0cd6c79b311797a2545644a10ce94180d",
            library_path_in_archive: "bin/pdfium.dll",
            library_file_name: "pdfium.dll",
        },
        "x86_64-unknown-linux-gnu" => PdfiumAsset {
            target: "x86_64-unknown-linux-gnu",
            asset_name: "pdfium-linux-x64.tgz",
            archive_sha256: "c30e092dc491b74bb666e6d35cd8d126102dad90fa87a722e16b312a2cd66c52",
            library_path_in_archive: "lib/libpdfium.so",
            library_file_name: "libpdfium.so",
        },
        _ => panic!("unsupported PDFium target: {target}"),
    }
}

fn download_pdfium(asset: &PdfiumAsset) -> Vec<u8> {
    let url = format!(
        "https://github.com/bblanchon/pdfium-binaries/releases/download/{}/{}",
        PDFIUM_RELEASE_URL_COMPONENT, asset.asset_name
    );

    println!(
        "Downloading PDFium {} ({}) from {url}",
        PDFIUM_RELEASE, asset.target
    );

    reqwest::blocking::get(url)
        .and_then(|response| response.error_for_status())
        .expect("download PDFium archive")
        .bytes()
        .expect("read PDFium archive")
        .to_vec()
}

fn verify_sha256(bytes: &[u8], expected: &str, asset_name: &str) {
    let actual = hex::encode(Sha256::digest(bytes));
    // Names the asset, not the dependency: this message once said "PDFium
    // archive" for an unrelated asset and sent the diagnosis to the wrong
    // place.
    assert_eq!(
        actual, expected,
        "SHA-256 mismatch for downloaded archive {asset_name}"
    );
}

fn extract_library(archive: &[u8], asset: &PdfiumAsset, destination: &Path) {
    let decoder = GzDecoder::new(Cursor::new(archive));
    let mut archive = tar::Archive::new(decoder);

    for entry in archive.entries().expect("read PDFium archive entries") {
        let mut entry = entry.expect("read PDFium archive entry");
        let path = entry.path().expect("read PDFium entry path");
        if path == Path::new(asset.library_path_in_archive) {
            entry
                .unpack(destination)
                .expect("unpack PDFium dynamic library");
            return;
        }
    }

    panic!(
        "PDFium library {} not found in {}",
        asset.library_path_in_archive, asset.asset_name
    );
}

/// Copy the tracked third-party notices to where the bundler can see them.
///
/// `<repo>/LICENSES` is the tracked copy README.md points a human at.
/// `src-tauri/resources/LICENSES` is the one that actually *ships*: the
/// `resources/**/*` glob in tauri.conf.json resolves relative to `src-tauri/`,
/// so while the repo copy was the only copy, every bundle went out carrying
/// third-party binaries with no licence text in it at all.
///
/// Runs on **every** build, deliberately. The obvious place for this is
/// alongside `extract_license_files` -- but that only runs on a cache miss, so
/// on any machine that already had PDFium unpacked (which is every machine
/// after the first build, and every CI run that restores the asset cache) the
/// notices would silently not ship. That is exactly how this shipped broken:
/// correct-looking code on a path that usually does not execute.
fn mirror_licenses_into_bundle(manifest_dir: &Path) {
    let source = manifest_dir.join("..").join("LICENSES");
    let destination = manifest_dir.join("resources").join("LICENSES");
    fs::create_dir_all(&destination).expect("create bundled LICENSES directory");

    let entries = fs::read_dir(&source).expect("read LICENSES directory");
    let mut copied = 0;
    for entry in entries {
        let entry = entry.expect("read LICENSES entry");
        if entry.file_type().expect("LICENSES entry type").is_file() {
            fs::copy(entry.path(), destination.join(entry.file_name()))
                .expect("copy licence notice into the bundle");
            copied += 1;
        }
    }
    assert!(
        copied > 0,
        "no licence notices found in {} -- the bundle would ship without them",
        source.display()
    );
    println!("cargo:rerun-if-changed={}", source.display());
}

fn extract_license_files(archive: &[u8], manifest_dir: &Path) {
    let licenses_dir = manifest_dir.join("..").join("LICENSES");
    fs::create_dir_all(&licenses_dir).expect("create LICENSES directory");

    let decoder = GzDecoder::new(Cursor::new(archive));
    let mut archive = tar::Archive::new(decoder);

    for entry in archive.entries().expect("read PDFium license entries") {
        let mut entry = entry.expect("read PDFium license entry");
        let path = entry.path().expect("read PDFium license path");
        if path == Path::new("licenses/pdfium.txt") || path == Path::new("LICENSE") {
            let filename = if path == Path::new("LICENSE") {
                "pdfium-binaries-license.txt"
            } else {
                "pdfium.txt"
            };
            entry
                .unpack(licenses_dir.join(filename))
                .expect("unpack PDFium license file");
        }
    }
}
