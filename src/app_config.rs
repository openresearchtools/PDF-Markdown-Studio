use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const DEFAULT_VLM_PROMPT: &str = "Convert this page to markdown. Do not miss any text and only output the bare markdown! Any graphs or figures found convert to markdown table. If figure is image without details, describe what you see in the image. For tables, pay attention to whitespace: some cells may be intentionally empty, so keep empty and filled cells in the correct columns. Ensure correct assignment of column headings and subheadings for tables.";
pub const DEFAULT_IMAGE_VLM_PROMPT: &str = "Describe this image in high detail and output clean markdown. Include all visible text exactly, preserve structure, and use markdown tables when tabular content is present. For charts or figures, summarize key visual elements and values if readable.";

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub base_config_dir: PathBuf,
    pub base_data_dir: PathBuf,
    pub app_config_dir: PathBuf,
    pub app_data_dir: PathBuf,
    pub runtime_shared_dir: PathBuf,
    pub models_dir: PathBuf,
    pub settings_json: PathBuf,
    pub manifest_cache_dir: PathBuf,
}

fn env_path(name: &str) -> Option<PathBuf> {
    env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

#[cfg(target_os = "windows")]
fn openresearchtools_roots() -> Result<(PathBuf, PathBuf), String> {
    if let Some(appdata) = env_path("APPDATA") {
        let root = appdata.join("OpenResearchTools");
        return Ok((root.clone(), root));
    }
    if let Some(local_appdata) = env_path("LOCALAPPDATA") {
        let root = local_appdata.join("OpenResearchTools");
        return Ok((root.clone(), root));
    }
    if let Some(user_profile) = env_path("USERPROFILE") {
        let root = user_profile
            .join("AppData")
            .join("Roaming")
            .join("OpenResearchTools");
        return Ok((root.clone(), root));
    }
    Err(
        "Neither APPDATA nor LOCALAPPDATA nor USERPROFILE is set; cannot resolve OpenResearchTools paths"
            .to_owned(),
    )
}

#[cfg(target_os = "macos")]
fn openresearchtools_roots() -> Result<(PathBuf, PathBuf), String> {
    let home = env_path("HOME").ok_or_else(|| {
        "HOME is not set; cannot resolve OpenResearchTools paths on macOS".to_owned()
    })?;
    let root = home
        .join("Library")
        .join("Application Support")
        .join("OpenResearchTools");
    Ok((root.clone(), root))
}

#[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
fn openresearchtools_roots() -> Result<(PathBuf, PathBuf), String> {
    let home = env_path("HOME").ok_or_else(|| {
        "HOME is not set; cannot resolve OpenResearchTools paths on this platform".to_owned()
    })?;
    let config_base = env_path("XDG_CONFIG_HOME").unwrap_or_else(|| home.join(".config"));
    let data_base = env_path("XDG_DATA_HOME").unwrap_or_else(|| home.join(".local").join("share"));
    let config_root = config_base.join("OpenResearchTools");
    let data_root = data_base.join("OpenResearchTools");
    Ok((config_root, data_root))
}

fn normalized_path_key(path: &Path) -> String {
    let raw = path.to_string_lossy();
    if cfg!(target_os = "windows") {
        raw.replace('/', "\\").to_ascii_lowercase()
    } else {
        raw.to_string()
    }
}

fn runtime_dir_is_legacy_default(path: &Path, paths: &AppPaths) -> bool {
    let legacy = paths.base_config_dir.join("engine-runtime");
    normalized_path_key(path) == normalized_path_key(&legacy)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ConversionMode {
    #[default]
    FastPdf,
    PdfVlm,
    FastPdfWithVlmFallback,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub runtime_dir: String,
    pub runtime_download_backend: String,
    pub conversion_mode: ConversionMode,
    pub vlm_model_path: String,
    pub vlm_mmproj_path: String,
    pub vlm_prompt: String,
    pub vlm_image_prompt: String,
    pub devices: String,
    pub n_ctx: i32,
    pub n_batch: i32,
    pub n_ubatch: i32,
    pub n_parallel: i32,
    pub n_threads: i32,
    pub n_threads_batch: i32,
    pub n_predict: i32,
    pub model_download_url: String,
    pub model_download_file_name: String,
    pub pdf_zoom: f32,
    pub markdown_font_size: f32,
}

impl Default for AppSettings {
    fn default() -> Self {
        let runtime_download_backend = if cfg!(target_os = "windows") {
            "vulkan"
        } else if cfg!(target_os = "macos") {
            "metal"
        } else {
            "vulkan"
        };
        Self {
            runtime_dir: String::new(),
            runtime_download_backend: runtime_download_backend.to_owned(),
            conversion_mode: ConversionMode::FastPdf,
            vlm_model_path: String::new(),
            vlm_mmproj_path: String::new(),
            vlm_prompt: DEFAULT_VLM_PROMPT.to_owned(),
            vlm_image_prompt: DEFAULT_IMAGE_VLM_PROMPT.to_owned(),
            devices: "none".to_owned(),
            n_ctx: 32768,
            n_batch: 2048,
            n_ubatch: 2048,
            n_parallel: 1,
            n_threads: 0,
            n_threads_batch: 0,
            n_predict: 5000,
            model_download_url: String::new(),
            model_download_file_name: String::new(),
            pdf_zoom: 1.0,
            markdown_font_size: 15.0,
        }
    }
}

pub fn app_paths() -> Result<AppPaths, String> {
    let (ort_config_root, ort_data_root) = openresearchtools_roots()?;
    let app_config_dir = ort_config_root.join("PDF Markdown Studio");
    let app_data_dir = ort_data_root.join("PDF Markdown Studio");
    let runtime_shared_dir = ort_data_root.join("engine");
    let models_dir = ort_data_root.join("Models").join("Vision");
    let manifest_cache_dir = app_config_dir.join("runtime-manifests");
    let settings_json = app_config_dir.join("settings.json");

    Ok(AppPaths {
        base_config_dir: ort_config_root,
        base_data_dir: ort_data_root,
        app_config_dir,
        app_data_dir,
        runtime_shared_dir,
        models_dir,
        settings_json,
        manifest_cache_dir,
    })
}

pub fn ensure_dirs(paths: &AppPaths) -> Result<(), String> {
    for dir in [
        &paths.base_config_dir,
        &paths.base_data_dir,
        &paths.app_config_dir,
        &paths.app_data_dir,
        &paths.runtime_shared_dir,
        &paths.models_dir,
        &paths.manifest_cache_dir,
    ] {
        fs::create_dir_all(dir)
            .map_err(|err| format!("failed to create '{}': {err}", dir.display()))?;
    }
    Ok(())
}

pub fn default_runtime_dir(paths: &AppPaths) -> PathBuf {
    paths.runtime_shared_dir.clone()
}

pub fn runtime_dir_from_settings(settings: &AppSettings, paths: &AppPaths) -> PathBuf {
    let trimmed = settings.runtime_dir.trim();
    if trimmed.is_empty() {
        default_runtime_dir(paths)
    } else {
        PathBuf::from(trimmed)
    }
}

pub fn load_settings(paths: &AppPaths) -> Result<AppSettings, String> {
    if !paths.settings_json.exists() {
        let mut initial = AppSettings::default();
        initial.runtime_dir = default_runtime_dir(paths).display().to_string();
        if initial.vlm_prompt.trim().is_empty() {
            initial.vlm_prompt = DEFAULT_VLM_PROMPT.to_owned();
        }
        if initial.vlm_image_prompt.trim().is_empty() {
            initial.vlm_image_prompt = DEFAULT_IMAGE_VLM_PROMPT.to_owned();
        }
        save_settings(paths, &initial)?;
        return Ok(initial);
    }

    let raw = fs::read_to_string(&paths.settings_json)
        .map_err(|err| format!("failed to read '{}': {err}", paths.settings_json.display()))?;

    let mut parsed: AppSettings = serde_json::from_str(&raw)
        .map_err(|err| format!("failed to parse '{}': {err}", paths.settings_json.display()))?;

    let mut runtime_path_migrated = false;
    let mut markdown_font_size_migrated = false;
    if parsed.runtime_dir.trim().is_empty() {
        parsed.runtime_dir = default_runtime_dir(paths).display().to_string();
    } else {
        let configured = PathBuf::from(parsed.runtime_dir.trim());
        if runtime_dir_is_legacy_default(&configured, paths) {
            parsed.runtime_dir = default_runtime_dir(paths).display().to_string();
            runtime_path_migrated = true;
        }
    }
    if parsed.vlm_prompt.trim().is_empty() {
        parsed.vlm_prompt = DEFAULT_VLM_PROMPT.to_owned();
    }
    if parsed.runtime_download_backend.trim().is_empty() {
        parsed.runtime_download_backend = if cfg!(target_os = "windows") {
            "vulkan".to_owned()
        } else if cfg!(target_os = "macos") {
            "metal".to_owned()
        } else {
            "vulkan".to_owned()
        };
    } else {
        parsed.runtime_download_backend =
            parsed.runtime_download_backend.trim().to_ascii_lowercase();
    }
    if parsed.vlm_image_prompt.trim().is_empty() {
        parsed.vlm_image_prompt = DEFAULT_IMAGE_VLM_PROMPT.to_owned();
    }
    if parsed.devices.trim().is_empty() {
        parsed.devices = "none".to_owned();
    }
    if parsed.n_ctx <= 0 {
        parsed.n_ctx = 32768;
    }
    if parsed.n_batch <= 0 {
        parsed.n_batch = 2048;
    }
    if parsed.n_ubatch <= 0 {
        parsed.n_ubatch = 2048;
    }
    if parsed.n_parallel <= 0 {
        parsed.n_parallel = 1;
    }
    if parsed.n_predict <= 0 {
        parsed.n_predict = 5000;
    }
    if parsed.pdf_zoom <= 0.0 {
        parsed.pdf_zoom = 1.0;
    }
    if parsed.markdown_font_size <= 0.0 {
        parsed.markdown_font_size = 15.0;
    }
    // Migrate legacy defaults (12pt, 16pt) to the current default (15pt).
    if (parsed.markdown_font_size - 12.0).abs() < f32::EPSILON
        || (parsed.markdown_font_size - 16.0).abs() < f32::EPSILON
    {
        parsed.markdown_font_size = 15.0;
        markdown_font_size_migrated = true;
    }

    if runtime_path_migrated || markdown_font_size_migrated {
        save_settings(paths, &parsed)?;
    }

    Ok(parsed)
}

pub fn save_settings(paths: &AppPaths, settings: &AppSettings) -> Result<(), String> {
    if let Some(parent) = paths.settings_json.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create '{}': {err}", parent.display()))?;
    }

    let raw = serde_json::to_string_pretty(settings)
        .map_err(|err| format!("failed to serialize settings: {err}"))?;
    fs::write(&paths.settings_json, raw)
        .map_err(|err| format!("failed to write '{}': {err}", paths.settings_json.display()))?;
    Ok(())
}

pub fn is_pdf(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"))
}
