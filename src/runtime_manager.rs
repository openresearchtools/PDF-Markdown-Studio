use std::env;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use flate2::read::GzDecoder;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tar::Archive;
use zip::ZipArchive;

use crate::app_config::AppPaths;

pub const DEFAULT_MANIFEST_URL: &str =
    "https://github.com/openresearchtools/engine/releases/latest/download/engine-manifest.json";

const APP_UA: &str = "PDFConverter/1.0";

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EngineManifest {
    #[serde(default)]
    pub schema_version: i32,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub repository: String,
    #[serde(default)]
    pub tag: String,
    #[serde(default)]
    pub generated_at: String,
    #[serde(default)]
    pub release_url: String,
    #[serde(default)]
    pub assets: Vec<ManifestAsset>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ManifestAsset {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub platform: String,
    #[serde(default)]
    pub backend: String,
    #[serde(default)]
    pub archive: String,
    #[serde(default)]
    pub file_name: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub sha256: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ManifestSources {
    #[serde(default)]
    sources: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct RuntimeCheck {
    pub missing: Vec<String>,
}

impl RuntimeCheck {
    pub fn is_ok(&self) -> bool {
        self.missing.is_empty()
    }
}

pub fn current_platform_key() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows-x64"
    } else if cfg!(target_os = "macos") {
        "macos-arm64"
    } else {
        "ubuntu-x64"
    }
}

fn bridge_library_file_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "llama-server-bridge.dll"
    } else if cfg!(target_os = "macos") {
        "libllama-server-bridge.dylib"
    } else {
        "libllama-server-bridge.so"
    }
}

fn pdfium_library_file_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "pdfium.dll"
    } else if cfg!(target_os = "macos") {
        "libpdfium.dylib"
    } else {
        "libpdfium.so"
    }
}

fn has_prefixed_file(dir: &Path, prefix: &str, suffix: &str) -> bool {
    let Ok(read_dir) = fs::read_dir(dir) else {
        return false;
    };
    for entry in read_dir.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(prefix) && name.ends_with(suffix) {
            return true;
        }
    }
    false
}

pub fn check_runtime_dir(runtime_dir: &Path) -> RuntimeCheck {
    let mut check = RuntimeCheck::default();

    if !runtime_dir.exists() {
        check.missing.push(format!(
            "Runtime directory does not exist: {}",
            runtime_dir.display()
        ));
        return check;
    }

    let bridge = runtime_dir.join(bridge_library_file_name());
    if !bridge.exists() {
        check.missing.push(format!(
            "Missing bridge library: {}",
            bridge_library_file_name()
        ));
    }

    let pdf_dll_name = if cfg!(target_os = "windows") {
        "pdf.dll"
    } else if cfg!(target_os = "macos") {
        "libpdf.dylib"
    } else {
        "libpdf.so"
    };
    let pdfvlm_dll_name = if cfg!(target_os = "windows") {
        "pdfvlm.dll"
    } else if cfg!(target_os = "macos") {
        "libpdfvlm.dylib"
    } else {
        "libpdfvlm.so"
    };

    if !runtime_dir.join(pdf_dll_name).exists() {
        check.missing.push(format!(
            "Missing fast PDF converter library: {pdf_dll_name}"
        ));
    }
    if !runtime_dir.join(pdfvlm_dll_name).exists() {
        check.missing.push(format!(
            "Missing VLM PDF converter library: {pdfvlm_dll_name}"
        ));
    }

    let pdfium = runtime_dir
        .join("vendor")
        .join("pdfium")
        .join(pdfium_library_file_name());
    if !pdfium.exists() {
        check.missing.push(format!(
            "Missing PDFium runtime: vendor/pdfium/{}",
            pdfium_library_file_name()
        ));
    }

    let ffmpeg_dir = if cfg!(target_os = "windows") {
        runtime_dir.join("vendor").join("ffmpeg").join("bin")
    } else {
        runtime_dir.join("vendor").join("ffmpeg").join("lib")
    };
    if !ffmpeg_dir.exists() {
        check.missing.push(format!(
            "Missing FFmpeg runtime directory: {}",
            ffmpeg_dir.display()
        ));
    } else if cfg!(target_os = "windows") {
        for prefix in ["avcodec", "avformat", "avutil", "swresample", "swscale"] {
            if !has_prefixed_file(&ffmpeg_dir, prefix, ".dll") {
                check
                    .missing
                    .push(format!("Missing FFmpeg DLL {}*.dll", prefix));
            }
        }
    } else if cfg!(target_os = "macos") {
        for prefix in [
            "libavcodec",
            "libavformat",
            "libavutil",
            "libswresample",
            "libswscale",
        ] {
            if !has_prefixed_file(&ffmpeg_dir, prefix, ".dylib") {
                check
                    .missing
                    .push(format!("Missing FFmpeg dylib {}*.dylib", prefix));
            }
        }
    } else {
        for prefix in [
            "libavcodec",
            "libavformat",
            "libavutil",
            "libswresample",
            "libswscale",
        ] {
            if !(has_prefixed_file(&ffmpeg_dir, prefix, ".so")
                || has_prefixed_file(&ffmpeg_dir, prefix, ".so.0")
                || has_prefixed_file(&ffmpeg_dir, prefix, ".so.1")
                || has_prefixed_file(&ffmpeg_dir, prefix, ".so.2"))
            {
                let exists = fs::read_dir(&ffmpeg_dir)
                    .ok()
                    .into_iter()
                    .flat_map(|entries| entries.flatten())
                    .any(|entry| entry.file_name().to_string_lossy().starts_with(prefix));
                if !exists {
                    check
                        .missing
                        .push(format!("Missing FFmpeg shared object {}*.so*", prefix));
                }
            }
        }
    }

    check
}

pub fn detect_installed_runtime_backend(runtime_dir: &Path) -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        if !runtime_dir.exists() {
            return None;
        }
        let candidate_dirs = [
            runtime_dir.to_path_buf(),
            runtime_dir.join("vendor"),
            runtime_dir.join("vendor").join("cuda"),
            runtime_dir.join("vendor").join("cuda").join("bin"),
        ];
        let has_cuda = candidate_dirs.iter().any(|dir| {
            has_prefixed_file(dir, "cublas", ".dll")
                || has_prefixed_file(dir, "cublasLt", ".dll")
                || has_prefixed_file(dir, "cudart", ".dll")
        });
        Some(if has_cuda { "cuda" } else { "vulkan" }.to_owned())
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = runtime_dir;
        None
    }
}

fn manifest_file_candidates(paths: &AppPaths) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(exe_path) = env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            out.push(
                exe_dir
                    .join("runtime-manifests")
                    .join("engine-manifest.json"),
            );
        }
    }
    out.push(paths.manifest_cache_dir.join("engine-manifest.json"));
    dedupe_paths(out)
}

fn manifest_sources_candidates(paths: &AppPaths) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(exe_path) = env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            out.push(
                exe_dir
                    .join("runtime-manifests")
                    .join("engine-manifest-sources.json"),
            );
        }
    }
    out.push(
        paths
            .manifest_cache_dir
            .join("engine-manifest-sources.json"),
    );
    dedupe_paths(out)
}

fn dedupe_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for path in paths {
        let key = path.to_string_lossy().to_ascii_lowercase();
        if out
            .iter()
            .any(|existing| existing.to_string_lossy().to_ascii_lowercase() == key)
        {
            continue;
        }
        out.push(path);
    }
    out
}

fn load_manifest_sources(paths: &AppPaths) -> Vec<String> {
    let mut out = vec![DEFAULT_MANIFEST_URL.to_owned()];
    for candidate in manifest_sources_candidates(paths) {
        if !candidate.exists() {
            continue;
        }
        let Ok(raw) = fs::read_to_string(&candidate) else {
            continue;
        };
        let Ok(parsed) = serde_json::from_str::<ManifestSources>(&raw) else {
            continue;
        };
        for source in parsed.sources {
            let source = source.trim();
            if source.is_empty() {
                continue;
            }
            if !out.iter().any(|known| known.eq_ignore_ascii_case(source)) {
                out.push(source.to_owned());
            }
        }
    }
    out
}

fn parse_manifest(raw: &str) -> Result<EngineManifest, String> {
    let manifest: EngineManifest =
        serde_json::from_str(raw).map_err(|err| format!("invalid engine manifest json: {err}"))?;
    if manifest.assets.is_empty() {
        return Err("engine manifest has no assets".to_owned());
    }
    Ok(manifest)
}

pub fn load_engine_manifest(paths: &AppPaths) -> Result<EngineManifest, String> {
    let mut errors = Vec::new();

    for file in manifest_file_candidates(paths) {
        if !file.exists() {
            continue;
        }
        match fs::read_to_string(&file) {
            Ok(raw) => match parse_manifest(&raw) {
                Ok(manifest) => return Ok(manifest),
                Err(err) => errors.push(format!("{}: {err}", file.display())),
            },
            Err(err) => errors.push(format!("{}: {err}", file.display())),
        }
    }

    let client = Client::builder()
        .user_agent(APP_UA)
        .timeout(Duration::from_secs(25))
        .build()
        .map_err(|err| format!("failed to build HTTP client: {err}"))?;

    for url in load_manifest_sources(paths) {
        if url.trim().is_empty() {
            continue;
        }
        match client.get(&url).send() {
            Ok(resp) => {
                let resp = match resp.error_for_status() {
                    Ok(value) => value,
                    Err(err) => {
                        errors.push(format!("{url}: {err}"));
                        continue;
                    }
                };
                let text = match resp.text() {
                    Ok(value) => value,
                    Err(err) => {
                        errors.push(format!("{url}: {err}"));
                        continue;
                    }
                };
                match parse_manifest(&text) {
                    Ok(manifest) => {
                        let cache_file = paths.manifest_cache_dir.join("engine-manifest.json");
                        let _ = fs::create_dir_all(&paths.manifest_cache_dir);
                        let _ = fs::write(
                            &cache_file,
                            serde_json::to_string_pretty(&manifest).unwrap_or_default(),
                        );
                        return Ok(manifest);
                    }
                    Err(err) => errors.push(format!("{url}: {err}")),
                }
            }
            Err(err) => errors.push(format!("{url}: {err}")),
        }
    }

    Err(format!(
        "failed to load engine manifest from local or remote sources:\n{}",
        errors.join("\n")
    ))
}

pub fn filtered_assets_for_platform(manifest: &EngineManifest) -> Vec<ManifestAsset> {
    let platform = current_platform_key();
    let mut assets = manifest
        .assets
        .iter()
        .filter(|asset| asset.platform.eq_ignore_ascii_case(platform))
        .cloned()
        .collect::<Vec<_>>();
    assets.sort_by_key(|asset| backend_priority(&asset.backend));
    assets
}

fn backend_priority(backend: &str) -> usize {
    let backend = backend.to_ascii_lowercase();
    if cfg!(target_os = "windows") {
        match backend.as_str() {
            "vulkan" => 0,
            "cuda" => 1,
            _ => 9,
        }
    } else if cfg!(target_os = "macos") {
        match backend.as_str() {
            "metal" => 0,
            _ => 9,
        }
    } else {
        match backend.as_str() {
            "vulkan" => 0,
            _ => 9,
        }
    }
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    if bytes == 0 {
        return "0 B".to_owned();
    }

    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }

    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn download_file_with_progress(
    client: &Client,
    url: &str,
    destination: &Path,
    mut on_progress: impl FnMut(String),
) -> Result<(), String> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("failed creating '{}': {err}", parent.display()))?;
    }

    let mut response = client
        .get(url)
        .send()
        .map_err(|err| format!("download request failed: {url}: {err}"))?
        .error_for_status()
        .map_err(|err| format!("download request returned error status: {url}: {err}"))?;

    let total = response.content_length();
    let temp_file = destination.with_extension("download");
    let mut file = File::create(&temp_file)
        .map_err(|err| format!("failed to create '{}': {err}", temp_file.display()))?;

    let mut buf = vec![0_u8; 64 * 1024];
    let mut downloaded = 0_u64;
    let started = Instant::now();
    let mut last_emit = Instant::now();

    loop {
        let read = response
            .read(&mut buf)
            .map_err(|err| format!("failed reading response body: {err}"))?;
        if read == 0 {
            break;
        }
        file.write_all(&buf[..read])
            .map_err(|err| format!("failed writing '{}': {err}", temp_file.display()))?;
        downloaded += read as u64;

        if last_emit.elapsed() >= Duration::from_millis(300) {
            let elapsed = started.elapsed().as_secs_f64().max(0.001);
            let speed = (downloaded as f64 / elapsed) as u64;
            let status = if let Some(total) = total {
                let percent = if total > 0 {
                    (downloaded as f64 * 100.0 / total as f64).clamp(0.0, 100.0)
                } else {
                    0.0
                };
                format!(
                    "Downloading: {percent:.1}% ({} / {}) at {}/s",
                    human_bytes(downloaded),
                    human_bytes(total),
                    human_bytes(speed)
                )
            } else {
                format!(
                    "Downloading: {} at {}/s",
                    human_bytes(downloaded),
                    human_bytes(speed)
                )
            };
            on_progress(status);
            last_emit = Instant::now();
        }
    }

    if let Some(total) = total {
        let elapsed = started.elapsed().as_secs_f64().max(0.001);
        let speed = (downloaded as f64 / elapsed) as u64;
        let percent = if total > 0 {
            (downloaded as f64 * 100.0 / total as f64).clamp(0.0, 100.0)
        } else {
            100.0
        };
        on_progress(format!(
            "Downloading: {percent:.1}% ({} / {}) at {}/s",
            human_bytes(downloaded),
            human_bytes(total),
            human_bytes(speed)
        ));
    } else {
        let elapsed = started.elapsed().as_secs_f64().max(0.001);
        let speed = (downloaded as f64 / elapsed) as u64;
        on_progress(format!(
            "Downloading: {} at {}/s",
            human_bytes(downloaded),
            human_bytes(speed)
        ));
    }

    file.flush()
        .map_err(|err| format!("failed flushing '{}': {err}", temp_file.display()))?;

    fs::rename(&temp_file, destination).map_err(|err| {
        format!(
            "failed moving downloaded file '{}' to '{}': {err}",
            temp_file.display(),
            destination.display()
        )
    })?;
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file =
        File::open(path).map_err(|err| format!("failed opening '{}': {err}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0_u8; 64 * 1024];

    loop {
        let read = file
            .read(&mut buf)
            .map_err(|err| format!("failed reading '{}': {err}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

fn extract_zip_file(zip_path: &Path, out_dir: &Path) -> Result<(), String> {
    let file = File::open(zip_path)
        .map_err(|err| format!("failed opening '{}': {err}", zip_path.display()))?;
    let mut archive = ZipArchive::new(file)
        .map_err(|err| format!("failed to parse '{}': {err}", zip_path.display()))?;

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|err| format!("failed reading zip entry #{index}: {err}"))?;

        let Some(enclosed) = entry.enclosed_name().map(|path| path.to_path_buf()) else {
            continue;
        };
        let output_path = out_dir.join(enclosed);

        if entry.is_dir() {
            fs::create_dir_all(&output_path)
                .map_err(|err| format!("failed creating '{}': {err}", output_path.display()))?;
            continue;
        }

        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("failed creating '{}': {err}", parent.display()))?;
        }

        let mut output = File::create(&output_path)
            .map_err(|err| format!("failed creating '{}': {err}", output_path.display()))?;
        std::io::copy(&mut entry, &mut output)
            .map_err(|err| format!("failed extracting '{}': {err}", output_path.display()))?;
    }

    Ok(())
}

fn extract_tar_gz_file(tgz_path: &Path, out_dir: &Path) -> Result<(), String> {
    let file = File::open(tgz_path)
        .map_err(|err| format!("failed opening '{}': {err}", tgz_path.display()))?;
    let decoder = GzDecoder::new(file);
    let mut archive = Archive::new(decoder);
    archive
        .unpack(out_dir)
        .map_err(|err| format!("failed extracting '{}': {err}", tgz_path.display()))?;
    Ok(())
}

fn copy_dir_recursive(source: &Path, destination: &Path) -> Result<(), String> {
    fs::create_dir_all(destination)
        .map_err(|err| format!("failed creating '{}': {err}", destination.display()))?;

    for entry in fs::read_dir(source)
        .map_err(|err| format!("failed reading '{}': {err}", source.display()))?
    {
        let entry = entry.map_err(|err| format!("failed reading dir entry: {err}"))?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());

        let metadata = entry
            .metadata()
            .map_err(|err| format!("failed reading metadata '{}': {err}", source_path.display()))?;
        if metadata.is_dir() {
            copy_dir_recursive(&source_path, &destination_path)?;
        } else {
            if let Some(parent) = destination_path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|err| format!("failed creating '{}': {err}", parent.display()))?;
            }
            fs::copy(&source_path, &destination_path).map_err(|err| {
                format!(
                    "failed copying '{}' to '{}': {err}",
                    source_path.display(),
                    destination_path.display()
                )
            })?;
        }
    }

    Ok(())
}

fn locate_runtime_root(extract_dir: &Path) -> Option<PathBuf> {
    if check_runtime_dir(extract_dir).is_ok() {
        return Some(extract_dir.to_path_buf());
    }

    let entries = fs::read_dir(extract_dir).ok()?;
    let mut directories = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();

    if directories.len() != 1 {
        return None;
    }

    let candidate = directories.remove(0);
    if check_runtime_dir(&candidate).is_ok() {
        Some(candidate)
    } else {
        None
    }
}

pub fn install_runtime_asset(
    asset: &ManifestAsset,
    runtime_dir: &Path,
    mut on_status: impl FnMut(String),
) -> Result<(), String> {
    if asset.url.trim().is_empty() {
        return Err("selected runtime asset has empty URL".to_owned());
    }

    let client = Client::builder()
        .user_agent(APP_UA)
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|err| format!("failed building HTTP client: {err}"))?;

    let now_ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temp_root = env::temp_dir().join(format!("pdf-converter-runtime-{now_ns}"));
    fs::create_dir_all(&temp_root)
        .map_err(|err| format!("failed creating '{}': {err}", temp_root.display()))?;

    let archive_name = if asset.file_name.trim().is_empty() {
        if asset.archive.eq_ignore_ascii_case("tar.gz") {
            "engine.tar.gz".to_owned()
        } else {
            "engine.zip".to_owned()
        }
    } else {
        asset.file_name.clone()
    };

    let archive_path = temp_root.join(&archive_name);
    on_status(format!("Downloading runtime: {}", asset.url));
    download_file_with_progress(&client, &asset.url, &archive_path, |status| {
        on_status(status)
    })?;

    if !asset.sha256.trim().is_empty() {
        let actual_hash = sha256_file(&archive_path)?;
        if !actual_hash.eq_ignore_ascii_case(asset.sha256.trim()) {
            return Err(format!(
                "runtime archive SHA256 mismatch for '{}': expected {}, got {}",
                archive_name,
                asset.sha256.trim(),
                actual_hash
            ));
        }
    }

    let extract_dir = temp_root.join("extract");
    fs::create_dir_all(&extract_dir)
        .map_err(|err| format!("failed creating '{}': {err}", extract_dir.display()))?;

    let archive_kind = if asset.archive.eq_ignore_ascii_case("tar.gz")
        || archive_name.to_ascii_lowercase().ends_with(".tar.gz")
    {
        "tar.gz"
    } else {
        "zip"
    };

    on_status(format!("Extracting runtime archive ({archive_kind})..."));
    if archive_kind == "tar.gz" {
        extract_tar_gz_file(&archive_path, &extract_dir)?;
    } else {
        extract_zip_file(&archive_path, &extract_dir)?;
    }

    let source_root = locate_runtime_root(&extract_dir).unwrap_or_else(|| extract_dir.clone());

    if runtime_dir.exists() {
        fs::remove_dir_all(runtime_dir)
            .map_err(|err| format!("failed clearing '{}': {err}", runtime_dir.display()))?;
    }
    fs::create_dir_all(runtime_dir)
        .map_err(|err| format!("failed creating '{}': {err}", runtime_dir.display()))?;

    copy_dir_recursive(&source_root, runtime_dir)?;

    let check = check_runtime_dir(runtime_dir);
    if !check.is_ok() {
        return Err(format!(
            "installed runtime is incomplete:\n{}",
            check.missing.join("\n")
        ));
    }

    let _ = fs::remove_dir_all(&temp_root);
    Ok(())
}

pub fn download_model_to_file(
    url: &str,
    destination: &Path,
    mut on_status: impl FnMut(String),
) -> Result<(), String> {
    if url.trim().is_empty() {
        return Err("model download URL is empty".to_owned());
    }

    let client = Client::builder()
        .user_agent(APP_UA)
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|err| format!("failed building HTTP client: {err}"))?;

    on_status(format!("Downloading model from {url}"));
    download_file_with_progress(&client, url, destination, |status| on_status(status))
}
