use std::env;
use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::process::Command;

use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};
use xz2::read::XzDecoder;
use zip::ZipArchive;

const PDFIUM_RELEASE: &str = "chromium/7789";
const PDFIUM_RELEASE_URL_COMPONENT: &str = "chromium%2F7789";

struct PdfiumAsset {
    target: &'static str,
    asset_name: &'static str,
    archive_sha256: &'static str,
    library_path_in_archive: &'static str,
    library_file_name: &'static str,
}

#[derive(Clone, Copy)]
enum ArchiveKind {
    TarXz,
    Zip,
}

#[derive(Clone, Copy)]
enum FfmpegSource {
    BtbN,
    ColorsWindMac,
}

struct FfmpegAsset {
    target: &'static str,
    asset_name: &'static str,
    archive_sha256: &'static str,
    source: FfmpegSource,
    archive_kind: ArchiveKind,
    executable_name: &'static str,
    sidecar_file_name: &'static str,
}

fn main() {
    let pdfium_library = ensure_pdfium();
    println!(
        "cargo:rustc-env=PDFIUM_LIBRARY_PATH={}",
        pdfium_library.display()
    );

    let ffmpeg_sidecar = ensure_ffmpeg();
    println!(
        "cargo:rustc-env=FFMPEG_SIDECAR_PATH={}",
        ffmpeg_sidecar.display()
    );

    tauri_build::build()
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

    if library_path.exists()
        && fs::read_to_string(&marker_path).is_ok_and(|value| value.trim() == asset.archive_sha256)
    {
        return library_path;
    }

    fs::create_dir_all(&resource_dir).expect("create PDFium resource directory");

    let archive = download_pdfium(&asset);
    verify_sha256(&archive, asset.archive_sha256, asset.asset_name);
    extract_library(&archive, &asset, &library_path);
    extract_license_files(&archive, &manifest_dir);
    fs::write(marker_path, format!("{}\n", asset.archive_sha256)).expect("write PDFium marker");

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
    assert_eq!(
        actual, expected,
        "SHA-256 mismatch for downloaded PDFium archive {asset_name}"
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

fn ensure_ffmpeg() -> PathBuf {
    let target = env::var("TARGET").expect("TARGET must be set by Cargo");
    let asset = ffmpeg_asset_for_target(&target);
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let binaries_dir = manifest_dir.join("binaries");
    let sidecar_path = binaries_dir.join(asset.sidecar_file_name);
    let marker_path = binaries_dir.join(format!(".{}.sha256", asset.sidecar_file_name));

    println!("cargo:rerun-if-changed={}", marker_path.display());

    if sidecar_path.exists()
        && fs::read_to_string(&marker_path).is_ok_and(|value| value.trim() == asset.archive_sha256)
    {
        return sidecar_path;
    }

    fs::create_dir_all(&binaries_dir).expect("create ffmpeg binaries directory");

    let archive = download_ffmpeg(&asset);
    verify_sha256(&archive, asset.archive_sha256, asset.asset_name);
    extract_ffmpeg(&archive, &asset, &binaries_dir, &sidecar_path);
    write_ffmpeg_license(&manifest_dir);
    fs::write(marker_path, format!("{}\n", asset.archive_sha256)).expect("write ffmpeg marker");

    sidecar_path
}

fn ffmpeg_asset_for_target(target: &str) -> FfmpegAsset {
    match target {
        "aarch64-apple-darwin" => FfmpegAsset {
            target: "aarch64-apple-darwin",
            asset_name: "FFmpeg-shared-n5.0.1-OSX-arm64.zip",
            archive_sha256: "0555a3218069e6c9d6ebb40e0124bd4516f004208c825d29c67a146b776b64bc",
            source: FfmpegSource::ColorsWindMac,
            archive_kind: ArchiveKind::Zip,
            executable_name: "ffmpeg",
            sidecar_file_name: "ffmpeg-aarch64-apple-darwin",
        },
        "x86_64-apple-darwin" => FfmpegAsset {
            target: "x86_64-apple-darwin",
            asset_name: "FFmpeg_shared-n5.0.1-OSX-x86_64.zip",
            archive_sha256: "3ac3fb7c79227f9cfcf2db947a5a0e6081c939d56f13e65eb6b5dcf59742e836",
            source: FfmpegSource::ColorsWindMac,
            archive_kind: ArchiveKind::Zip,
            executable_name: "ffmpeg",
            sidecar_file_name: "ffmpeg-x86_64-apple-darwin",
        },
        "x86_64-pc-windows-msvc" => FfmpegAsset {
            target: "x86_64-pc-windows-msvc",
            asset_name: "ffmpeg-master-latest-win64-lgpl-shared.zip",
            archive_sha256: "b5b73363a72f73da39463688be20cde0f17b626a544ab3c3c68ef44e24e31a6f",
            source: FfmpegSource::BtbN,
            archive_kind: ArchiveKind::Zip,
            executable_name: "ffmpeg.exe",
            sidecar_file_name: "ffmpeg-x86_64-pc-windows-msvc.exe",
        },
        "x86_64-unknown-linux-gnu" => FfmpegAsset {
            target: "x86_64-unknown-linux-gnu",
            asset_name: "ffmpeg-master-latest-linux64-lgpl-shared.tar.xz",
            archive_sha256: "171aa349c6e1d018d602ecdf497d493fa9aa7c84f9d1d0160526f23f331d37d3",
            source: FfmpegSource::BtbN,
            archive_kind: ArchiveKind::TarXz,
            executable_name: "ffmpeg",
            sidecar_file_name: "ffmpeg-x86_64-unknown-linux-gnu",
        },
        "aarch64-unknown-linux-gnu" => FfmpegAsset {
            target: "aarch64-unknown-linux-gnu",
            asset_name: "ffmpeg-master-latest-linuxarm64-lgpl-shared.tar.xz",
            archive_sha256: "4537a74aca76d66faf6d04d82a9b0fc8b4b8185ced17def66afbe683c4fe95c7",
            source: FfmpegSource::BtbN,
            archive_kind: ArchiveKind::TarXz,
            executable_name: "ffmpeg",
            sidecar_file_name: "ffmpeg-aarch64-unknown-linux-gnu",
        },
        _ => panic!("unsupported ffmpeg target: {target}"),
    }
}

fn download_ffmpeg(asset: &FfmpegAsset) -> Vec<u8> {
    let url = match asset.source {
        FfmpegSource::BtbN => format!(
            "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/{}",
            asset.asset_name
        ),
        FfmpegSource::ColorsWindMac => format!(
            "https://github.com/ColorsWind/FFmpeg-macOS/releases/download/n5.0.1-patch3/{}",
            asset.asset_name
        ),
    };

    println!("Downloading ffmpeg ({}) from {url}", asset.target);

    reqwest::blocking::get(url)
        .and_then(|response| response.error_for_status())
        .expect("download ffmpeg archive")
        .bytes()
        .expect("read ffmpeg archive")
        .to_vec()
}

fn extract_ffmpeg(archive: &[u8], asset: &FfmpegAsset, binaries_dir: &Path, sidecar_path: &Path) {
    let libs_dir = binaries_dir.join(format!("{}-libs", asset.sidecar_file_name));
    if libs_dir.exists() {
        fs::remove_dir_all(&libs_dir).expect("remove old ffmpeg library directory");
    }

    match asset.archive_kind {
        ArchiveKind::Zip => {
            extract_ffmpeg_zip(archive, asset, binaries_dir, sidecar_path, &libs_dir)
        }
        ArchiveKind::TarXz => {
            let decoder = XzDecoder::new(Cursor::new(archive));
            extract_ffmpeg_tar(decoder, asset, sidecar_path, &libs_dir);
        }
    }

    make_executable(sidecar_path);

    if cfg!(target_os = "macos") {
        patch_macos_ffmpeg(asset, sidecar_path, &libs_dir);
    }
}

fn extract_ffmpeg_zip(
    archive: &[u8],
    asset: &FfmpegAsset,
    binaries_dir: &Path,
    sidecar_path: &Path,
    libs_dir: &Path,
) {
    let mut archive = ZipArchive::new(Cursor::new(archive)).expect("open ffmpeg zip archive");
    let mut found_executable = false;

    for index in 0..archive.len() {
        let mut file = archive.by_index(index).expect("read ffmpeg zip entry");
        if !file.is_file() {
            continue;
        }

        let path = PathBuf::from(file.name());
        let path_text = path.to_string_lossy();
        if path.ends_with(Path::new("bin").join(asset.executable_name)) {
            copy_reader_to_path(&mut file, sidecar_path);
            found_executable = true;
        } else if matches!(asset.source, FfmpegSource::ColorsWindMac)
            && path_text.starts_with("lib/")
            && path_text.ends_with(".dylib")
        {
            let file_name = path.file_name().expect("dylib file name");
            copy_reader_to_path(&mut file, &libs_dir.join(file_name));
        } else if asset.target.contains("windows") && path_text.ends_with(".dll") {
            let file_name = path.file_name().expect("dll file name");
            copy_reader_to_path(&mut file, &binaries_dir.join(file_name));
        }
    }

    assert!(
        found_executable,
        "ffmpeg executable {} not found in {}",
        asset.executable_name, asset.asset_name
    );
}

fn extract_ffmpeg_tar<R: Read>(
    reader: R,
    asset: &FfmpegAsset,
    sidecar_path: &Path,
    libs_dir: &Path,
) {
    let mut archive = tar::Archive::new(reader);
    let mut found_executable = false;

    for entry in archive.entries().expect("read ffmpeg tar entries") {
        let mut entry = entry.expect("read ffmpeg tar entry");
        if !entry.header().entry_type().is_file() {
            continue;
        }

        let path = entry.path().expect("read ffmpeg tar entry path");
        let path_text = path.to_string_lossy();
        if path.ends_with(Path::new("bin").join(asset.executable_name)) {
            entry
                .unpack(sidecar_path)
                .expect("unpack ffmpeg executable");
            found_executable = true;
        } else if path_text.contains("/lib/") && path_text.contains(".so") {
            let file_name = path.file_name().expect("shared library file name");
            entry
                .unpack(libs_dir.join(file_name))
                .expect("unpack ffmpeg shared library");
        }
    }

    assert!(
        found_executable,
        "ffmpeg executable {} not found in {}",
        asset.executable_name, asset.asset_name
    );
}

fn copy_reader_to_path<R: Read>(reader: &mut R, destination: &Path) {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).expect("create destination directory");
    }

    let mut output = fs::File::create(destination).expect("create extracted file");
    std::io::copy(reader, &mut output).expect("copy extracted file");
}

fn make_executable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = fs::metadata(path).expect("ffmpeg metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).expect("set ffmpeg executable bit");
    }
}

fn patch_macos_ffmpeg(asset: &FfmpegAsset, sidecar_path: &Path, libs_dir: &Path) {
    if !libs_dir.exists() {
        return;
    }

    let libs_dir_name = libs_dir
        .file_name()
        .expect("ffmpeg lib directory name")
        .to_string_lossy();
    let mut patch_targets = vec![sidecar_path.to_path_buf()];
    for entry in fs::read_dir(libs_dir).expect("read ffmpeg dylib directory") {
        let entry = entry.expect("read ffmpeg dylib");
        if entry
            .path()
            .extension()
            .is_some_and(|extension| extension == "dylib")
        {
            patch_targets.push(entry.path());
        }
    }

    for target in &patch_targets {
        if target != sidecar_path {
            let dylib_name = target
                .file_name()
                .expect("dylib file name")
                .to_string_lossy();
            run_command(
                "install_name_tool",
                &[
                    "-id",
                    &format!("@executable_path/{libs_dir_name}/{dylib_name}"),
                ],
                target,
            );
        }

        let linked_libraries = linked_macos_libraries(target);
        for original_path in linked_libraries {
            let Some(library_name) = Path::new(&original_path).file_name() else {
                continue;
            };
            let replacement = format!(
                "@executable_path/{}/{}",
                libs_dir_name,
                library_name.to_string_lossy()
            );
            run_command(
                "install_name_tool",
                &["-change", original_path.as_str(), replacement.as_str()],
                target,
            );
        }
    }

    for target in &patch_targets {
        make_executable(target);
        run_command("codesign", &["--force", "--sign", "-"], target);
    }

    println!("Prepared macOS ffmpeg sidecar for {}", asset.target);
}

fn linked_macos_libraries(path: &Path) -> Vec<String> {
    let output = Command::new("otool")
        .arg("-L")
        .arg(path)
        .output()
        .expect("run otool");
    assert!(
        output.status.success(),
        "otool failed for {}",
        path.display()
    );

    String::from_utf8(output.stdout)
        .expect("utf8 otool output")
        .lines()
        .filter_map(|line| {
            let library = line.split_whitespace().next()?;
            if library.contains("/FFmpeg-macOS/") && library.ends_with(".dylib") {
                Some(library.to_string())
            } else {
                None
            }
        })
        .collect()
}

fn run_command(program: &str, args: &[&str], path: &Path) {
    let status = Command::new(program)
        .args(args)
        .arg(path)
        .status()
        .unwrap_or_else(|error| panic!("run {program} for {}: {error}", path.display()));
    assert!(
        status.success(),
        "{program} failed for {} with status {status}",
        path.display()
    );
}

fn write_ffmpeg_license(manifest_dir: &Path) {
    let licenses_dir = manifest_dir.join("..").join("LICENSES");
    fs::create_dir_all(&licenses_dir).expect("create LICENSES directory");
    fs::write(
        licenses_dir.join("ffmpeg.txt"),
        "FFmpeg is distributed under the GNU Lesser General Public License \
         version 2.1 or later, depending on build configuration. Johnny Reader \
         uses LGPL shared builds for the bundled sidecar.\n\n\
         Sources:\n\
         - https://github.com/BtbN/FFmpeg-Builds\n\
         - https://github.com/ColorsWind/FFmpeg-macOS\n\
         - https://ffmpeg.org/legal.html\n",
    )
    .expect("write ffmpeg license notice");
}
