#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod app_config;
mod engine_bindings;
mod runtime_manager;

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use chrono::Local;
use eframe::egui::{
    self, Align, Color32, ColorImage, CornerRadius, FontData, FontDefinitions, FontFamily, FontId,
    IconData, RichText, ScrollArea, Stroke, Vec2,
};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use pdfium_render::prelude::*;

use app_config::{
    AppPaths, AppSettings, ConversionMode, DEFAULT_IMAGE_VLM_PROMPT, DEFAULT_VLM_PROMPT, app_paths,
    ensure_dirs, load_settings, runtime_dir_from_settings, save_settings,
};
use engine_bindings::{
    EngineDevice, PdfVlmRequest, list_bridge_devices, run_image_vlm, run_pdf_fast, run_pdf_vlm,
    runtime_pdfium_library_path,
};
use runtime_manager::{
    EngineManifest, ManifestAsset, RuntimeCheck, check_runtime_dir,
    detect_installed_runtime_backend, filtered_assets_for_platform, install_runtime_asset,
    load_engine_manifest,
};

#[derive(Clone, Copy, Debug)]
struct ModelComboPreset {
    label: &'static str,
    requirement: &'static str,
    model_url: &'static str,
    model_file: &'static str,
    mmproj_url: &'static str,
    mmproj_file: &'static str,
    repo_url: &'static str,
    notes: &'static str,
}

const MODEL_COMBO_PRESETS: [ModelComboPreset; 3] = [
    ModelComboPreset {
        label: "Qwen3-VL-8B-Instruct Q8_0 (Recommended)",
        requirement: "~16 GB RAM/VRAM",
        model_url: "https://huggingface.co/Qwen/Qwen3-VL-8B-Instruct-GGUF/resolve/main/Qwen3VL-8B-Instruct-Q8_0.gguf?download=true",
        model_file: "Qwen3VL-8B-Instruct-Q8_0.gguf",
        mmproj_url: "https://huggingface.co/Qwen/Qwen3-VL-8B-Instruct-GGUF/resolve/main/mmproj-Qwen3VL-8B-Instruct-F16.gguf?download=true",
        mmproj_file: "mmproj-Qwen3VL-8B-Instruct-F16.gguf",
        repo_url: "https://huggingface.co/Qwen/Qwen3-VL-8B-Instruct-GGUF",
        notes: "Best quality preset. Typically comfortable with n_ctx=32768 and n_parallel=4 on a 16 GB VRAM GPU.",
    },
    ModelComboPreset {
        label: "Qwen3-VL-4B-Instruct Q8_0",
        requirement: "~12 GB RAM/VRAM",
        model_url: "https://huggingface.co/Qwen/Qwen3-VL-4B-Instruct-GGUF/resolve/main/Qwen3VL-4B-Instruct-Q8_0.gguf?download=true",
        model_file: "Qwen3VL-4B-Instruct-Q8_0.gguf",
        mmproj_url: "https://huggingface.co/Qwen/Qwen3-VL-4B-Instruct-GGUF/resolve/main/mmproj-Qwen3VL-4B-Instruct-F16.gguf?download=true",
        mmproj_file: "mmproj-Qwen3VL-4B-Instruct-F16.gguf",
        repo_url: "https://huggingface.co/Qwen/Qwen3-VL-4B-Instruct-GGUF",
        notes: "Balanced speed/quality preset for lower-memory devices.",
    },
    ModelComboPreset {
        label: "Qwen3-VL-2B-Instruct Q8_0",
        requirement: "~8 GB RAM/VRAM",
        model_url: "https://huggingface.co/Qwen/Qwen3-VL-2B-Instruct-GGUF/resolve/main/Qwen3VL-2B-Instruct-Q8_0.gguf?download=true",
        model_file: "Qwen3VL-2B-Instruct-Q8_0.gguf",
        mmproj_url: "https://huggingface.co/Qwen/Qwen3-VL-2B-Instruct-GGUF/resolve/main/mmproj-Qwen3VL-2B-Instruct-F16.gguf?download=true",
        mmproj_file: "mmproj-Qwen3VL-2B-Instruct-F16.gguf",
        repo_url: "https://huggingface.co/Qwen/Qwen3-VL-2B-Instruct-GGUF",
        notes: "Lowest-memory preset; useful when GPU/VRAM is limited.",
    },
];
const FAST_MACHINE_READABILITY_HINT_A: &str = "machine-readability gate rejected";
const FAST_MACHINE_READABILITY_HINT_B: &str = "content appears non-machine-readable";
const VLM_DEVICE_CPU_LABEL: &str = "CPU (no GPU)";
const APP_ID: &str = "pdf-markdown-studio";
const PDF_ZOOM_MIN: f32 = 0.55;
const PDF_ZOOM_MAX: f32 = 2.75;
const PDF_ZOOM_STEP: f32 = 0.10;
const PAGE_ACTIVE_VISIBILITY_THRESHOLD: f32 = 0.80;
const MAX_BACKGROUND_EVENTS_PER_FRAME: usize = 32;
const MAX_DOCUMENT_MATERIALIZATIONS_PER_FRAME: usize = 1;
const BACKGROUND_REPAINT_MS: u64 = 100;
#[cfg(target_os = "linux")]
const CONVERSION_ONLY_REPAINT_MS_LINUX: u64 = 750;
const BUNDLED_PDF_MARKDOWN_STUDIO_LICENSE_TXT: &str = include_str!("../LICENSE");
const BUNDLED_THIRD_PARTY_NOTICES_ALL_MD: &str =
    include_str!("../licenses/THIRD_PARTY_NOTICES_ALL.md");
const BUNDLED_THIRD_PARTY_LICENSES_ALL_MD: &str =
    include_str!("../licenses/THIRD_PARTY_LICENSES_ALL.md");
const BUNDLED_ENGINE_THIRD_PARTY_LICENSES_FULL_MD: &str =
    include_str!("../licenses/ENGINE_THIRD_PARTY_LICENSES_FULL.md");
const BUNDLED_UNBLOCK_UNSIGNED_RUNTIME_PS1: &str =
    include_str!("../scripts/unblock-unsigned-runtime.ps1");
const BUNDLED_UNBLOCK_UNSIGNED_RUNTIME_SH: &str =
    include_str!("../scripts/unblock-unsigned-runtime.sh");
const APP_ICON_PNG_BYTES: &[u8] = include_bytes!("../logo/windows/app_icon.png");

fn main() -> eframe::Result<()> {
    let viewport = with_app_icon(
        egui::ViewportBuilder::default()
            .with_title("PDF Markdown Studio")
            .with_app_id(APP_ID)
            .with_inner_size([1500.0, 900.0])
            .with_min_inner_size([960.0, 640.0])
            .with_maximized(true),
    );

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "PDF Markdown Studio",
        options,
        Box::new(|cc| Ok(Box::new(PdfMarkdownApp::new(cc)))),
    )
}

fn decode_app_icon(png_bytes: &[u8]) -> Option<IconData> {
    let image = image::load_from_memory(png_bytes).ok()?.to_rgba8();
    let width = image.width();
    let height = image.height();
    Some(IconData {
        width,
        height,
        rgba: image.into_raw(),
    })
}

fn with_app_icon(mut viewport: egui::ViewportBuilder) -> egui::ViewportBuilder {
    if let Some(icon) = decode_app_icon(APP_ICON_PNG_BYTES) {
        viewport = viewport.with_icon(icon);
    }
    viewport
}

#[derive(Clone, Debug, Default)]
struct PaneMetrics {
    hovered: bool,
    user_scrolled: bool,
    scroll_offset_y: f32,
    first_visible_page: Option<usize>,
}

#[derive(Clone, Copy, Debug, Default)]
struct MarkdownPaneUiActions {
    request_enter_edit_mode: bool,
    request_enter_edit_mode_page: Option<usize>,
    request_exit_edit_mode: bool,
    request_save_edits: bool,
}

enum MarkdownRenderBlock {
    Markdown(String),
    Table {
        rows: Vec<Vec<String>>,
        alignments: Vec<Align>,
    },
}

#[derive(Clone, Debug)]
struct SearchHit {
    page_index: usize,
    pdf_hits: usize,
    markdown_hits: usize,
    excerpt: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JobState {
    Pending,
    Running,
    Completed,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JobKind {
    Conversion,
    DocumentLoad,
    RuntimeInstall,
    ModelDownload,
    DeviceEnumeration,
}

impl JobKind {
    fn label(self) -> &'static str {
        match self {
            JobKind::Conversion => "Conversion",
            JobKind::DocumentLoad => "Load",
            JobKind::RuntimeInstall => "Runtime",
            JobKind::ModelDownload => "Model",
            JobKind::DeviceEnumeration => "Devices",
        }
    }
}

#[derive(Clone, Debug)]
struct JobRecord {
    id: u64,
    kind: JobKind,
    title: String,
    state: JobState,
    detail: String,
    progress_percent: Option<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LogLevel {
    Info,
    Warn,
    Error,
}

#[derive(Clone, Debug)]
struct LogEntry {
    timestamp: String,
    level: LogLevel,
    message: String,
}

#[derive(Clone, Debug)]
struct VlmDeviceOption {
    label: String,
    devices_value: String,
    main_gpu: i32,
    is_gpu: bool,
    detail_line: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ModelDownloadPurpose {
    Model,
    Mmproj,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LegalDocKind {
    ThirdPartyNotices,
    ThirdPartyLicenses,
    EngineThirdPartyLicenses,
}

impl LegalDocKind {
    const ALL: [Self; 3] = [
        Self::ThirdPartyNotices,
        Self::ThirdPartyLicenses,
        Self::EngineThirdPartyLicenses,
    ];

    fn nav_label(self) -> &'static str {
        match self {
            Self::ThirdPartyNotices => "Notices",
            Self::ThirdPartyLicenses => "Third-party licenses",
            Self::EngineThirdPartyLicenses => "Engine licenses",
        }
    }

    fn window_title(self) -> &'static str {
        match self {
            Self::ThirdPartyNotices => "Notices",
            Self::ThirdPartyLicenses => "Third-Party Licenses",
            Self::EngineThirdPartyLicenses => "Engine Licenses",
        }
    }

    fn bundled_markdown(self) -> &'static str {
        match self {
            Self::ThirdPartyNotices => BUNDLED_THIRD_PARTY_NOTICES_ALL_MD,
            Self::ThirdPartyLicenses => BUNDLED_THIRD_PARTY_LICENSES_ALL_MD,
            Self::EngineThirdPartyLicenses => BUNDLED_ENGINE_THIRD_PARTY_LICENSES_FULL_MD,
        }
    }
}

#[derive(Clone, Debug)]
enum ConversionTaskPayload {
    FastPdf {
        input_path: PathBuf,
        output_path: PathBuf,
    },
    PdfVlm {
        request: PdfVlmRequest,
    },
    FastPdfWithVlmFallback {
        input_path: PathBuf,
        fast_output_path: PathBuf,
        fallback_request: PdfVlmRequest,
    },
}

#[derive(Clone, Debug)]
struct ConversionTask {
    job_id: u64,
    doc_id: usize,
    doc_name: String,
    mode: ConversionMode,
    runtime_dir: PathBuf,
    payload: ConversionTaskPayload,
}

#[derive(Clone, Debug)]
struct ConversionTaskOutcome {
    markdown: String,
    output_path: PathBuf,
    used_mode: ConversionMode,
    fallback_used: bool,
    fast_error: Option<String>,
}

#[derive(Clone, Debug)]
struct LoadedRaster {
    width: usize,
    height: usize,
    rgba: Vec<u8>,
}

#[derive(Clone, Debug)]
struct LoadedPdfPagePayload {
    raster: LoadedRaster,
    text: String,
}

#[derive(Clone, Debug)]
enum LoadedDocumentKind {
    Pdf {
        pages: Vec<LoadedPdfPagePayload>,
        markdown: String,
    },
    Image {
        raster: LoadedRaster,
        markdown: String,
    },
}

#[derive(Clone, Debug)]
struct LoadedDocumentPayload {
    path: PathBuf,
    name: String,
    kind: LoadedDocumentKind,
}

enum BackgroundEvent {
    JobProgress {
        job_id: u64,
        status: String,
    },
    RuntimeInstalled {
        job_id: u64,
        result: Result<(), String>,
    },
    RuntimeUnblocked {
        job_id: u64,
        result: Result<String, String>,
    },
    DevicesEnumerated {
        job_id: u64,
        result: Result<Vec<EngineDevice>, String>,
    },
    ModelDownloaded {
        job_id: u64,
        purpose: ModelDownloadPurpose,
        result: Result<PathBuf, String>,
    },
    DocumentLoaded {
        job_id: u64,
        result: Result<LoadedDocumentPayload, String>,
    },
    ConversionFinished {
        job_id: u64,
        doc_id: usize,
        doc_name: String,
        requested_mode: ConversionMode,
        result: Result<ConversionTaskOutcome, String>,
    },
}

struct PdfPageData {
    texture: egui::TextureHandle,
    image_size: Vec2,
    text: String,
}

struct PdfDocumentData {
    pages: Vec<PdfPageData>,
}

struct ImageDocumentData {
    texture: egui::TextureHandle,
    image_size: Vec2,
}

enum DocumentKind {
    Pdf(PdfDocumentData),
    Image(ImageDocumentData),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MarkdownPreviewKind {
    Placeholder,
    Fast,
    Vlm,
}

impl MarkdownPreviewKind {
    fn label(self) -> &'static str {
        match self {
            MarkdownPreviewKind::Placeholder => "Pending",
            MarkdownPreviewKind::Fast => "FAST.md",
            MarkdownPreviewKind::Vlm => "VLM.md",
        }
    }
}

struct WorkspaceDocument {
    id: usize,
    path: PathBuf,
    name: String,
    markdown: String,
    markdown_edit_mode: bool,
    markdown_edit_baseline: Option<String>,
    selected_for_conversion: bool,
    fast_markdown_path: Option<PathBuf>,
    vlm_markdown_path: Option<PathBuf>,
    active_markdown_preview: MarkdownPreviewKind,
    kind: DocumentKind,
}

impl WorkspaceDocument {
    fn page_count(&self) -> usize {
        match &self.kind {
            DocumentKind::Pdf(data) => data.pages.len().max(1),
            DocumentKind::Image(_) => 1,
        }
    }

    fn doc_type_label(&self) -> &'static str {
        match self.kind {
            DocumentKind::Pdf(_) => "PDF",
            DocumentKind::Image(_) => "Image",
        }
    }
}

struct PdfMarkdownApp {
    paths: AppPaths,
    settings: AppSettings,
    documents: Vec<WorkspaceDocument>,
    selected_doc: Option<usize>,
    next_doc_id: usize,
    next_texture_id: usize,
    markdown_cache: CommonMarkCache,

    status_message: String,
    prompt_overrides_window_open: bool,
    about_window_open: bool,
    legal_docs_window_open: bool,
    legal_doc_kind: LegalDocKind,
    legal_doc_lines: Vec<String>,
    logs_window_open: bool,
    log_entries: Vec<LogEntry>,
    jobs: Vec<JobRecord>,
    next_job_id: u64,
    conversion_queue: VecDeque<ConversionTask>,
    active_conversion_job: Option<u64>,
    active_conversion_key: Option<(usize, ConversionMode)>,
    search_query: String,
    search_hits: Vec<SearchHit>,
    active_search_hit: usize,

    current_page: usize,
    page_input: String,
    pending_sync_to_pdf: Option<usize>,
    pending_sync_to_markdown: Option<usize>,
    last_split_view_size: Vec2,
    last_pdf_scroll_offset_y: f32,
    last_markdown_scroll_offset_y: f32,
    last_markdown_toggle_at: f64,
    pending_markdown_exit_doc_id: Option<usize>,
    pending_markdown_edit_focus_page: Option<usize>,

    runtime_check: RuntimeCheck,
    runtime_popup_open: bool,
    settings_window_open: bool,
    runtime_manifest: Option<EngineManifest>,
    runtime_assets: Vec<ManifestAsset>,
    selected_runtime_asset: usize,
    runtime_install_backends: Vec<String>,
    selected_runtime_install_backend: usize,
    runtime_download_in_progress: bool,
    runtime_unblock_in_progress: bool,
    runtime_post_install_prompt: bool,
    model_download_in_progress: bool,
    selected_model_combo_preset: usize,
    pending_mmproj_combo_preset: Option<usize>,
    device_enumeration_in_progress: bool,
    available_devices: Vec<EngineDevice>,
    last_enumerated_runtime_dir: Option<PathBuf>,
    device_options: Vec<VlmDeviceOption>,
    selected_device_option: usize,
    runtime_status: String,

    background_tx: Sender<BackgroundEvent>,
    background_rx: Receiver<BackgroundEvent>,
    pending_document_load_results: VecDeque<(u64, Result<LoadedDocumentPayload, String>)>,
}

impl PdfMarkdownApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        apply_modern_theme(&cc.egui_ctx);

        let (background_tx, background_rx) = mpsc::channel();
        let mut startup_status = "Ready.".to_owned();

        let paths = app_paths().unwrap_or_else(|err| {
            panic!("failed to resolve app directories for PDF Markdown Studio: {err}")
        });
        if let Err(err) = ensure_dirs(&paths) {
            startup_status = format!("Failed to create app directories: {err}");
        }

        let mut settings = match load_settings(&paths) {
            Ok(settings) => settings,
            Err(err) => {
                startup_status = format!("Failed to load settings: {err}");
                let mut defaults = AppSettings::default();
                defaults.runtime_dir = paths.runtime_shared_dir.display().to_string();
                defaults
            }
        };
        // Always start in fit mode; manual zoom can expand/shrink after launch.
        settings.pdf_zoom = 1.0;

        let runtime_dir = runtime_dir_from_settings(&settings, &paths);
        let runtime_check = check_runtime_dir(&runtime_dir);
        let runtime_popup_open = !runtime_check.is_ok()
            || !configured_path_exists(&settings.vlm_model_path)
            || !configured_path_exists(&settings.vlm_mmproj_path);

        let (runtime_manifest, runtime_assets, runtime_status) = match load_engine_manifest(&paths)
        {
            Ok(manifest) => {
                let assets = filtered_assets_for_platform(&manifest);
                let tag = if manifest.tag.trim().is_empty() {
                    "latest".to_owned()
                } else {
                    manifest.tag.clone()
                };
                (
                    Some(manifest),
                    assets,
                    format!("Runtime manifest loaded ({tag})."),
                )
            }
            Err(err) => (
                None,
                Vec::new(),
                format!("Runtime manifest not loaded: {err}"),
            ),
        };
        let runtime_install_backends = Self::runtime_backend_options_from_assets(&runtime_assets);
        let selected_runtime_install_backend = Self::resolve_runtime_backend_index(
            &runtime_install_backends,
            &settings.runtime_download_backend,
        )
        .min(runtime_install_backends.len().saturating_sub(1));
        if let Some(backend) = runtime_install_backends.get(selected_runtime_install_backend) {
            settings.runtime_download_backend = backend.clone();
        }
        let initial_device_options = Self::build_vlm_device_options(&[]);
        let initial_selected_device_option =
            Self::resolve_selected_device_index(&initial_device_options, &settings)
                .min(initial_device_options.len().saturating_sub(1));

        let mut app = Self {
            paths,
            settings,
            documents: Vec::new(),
            selected_doc: None,
            next_doc_id: 1,
            next_texture_id: 1,
            markdown_cache: CommonMarkCache::default(),
            status_message: startup_status,
            prompt_overrides_window_open: false,
            about_window_open: false,
            legal_docs_window_open: false,
            legal_doc_kind: LegalDocKind::ThirdPartyNotices,
            legal_doc_lines: LegalDocKind::ThirdPartyNotices
                .bundled_markdown()
                .lines()
                .map(|line| line.to_owned())
                .collect(),
            logs_window_open: false,
            log_entries: Vec::new(),
            jobs: Vec::new(),
            next_job_id: 1,
            conversion_queue: VecDeque::new(),
            active_conversion_job: None,
            active_conversion_key: None,
            search_query: String::new(),
            search_hits: Vec::new(),
            active_search_hit: 0,
            current_page: 0,
            page_input: "1".to_owned(),
            pending_sync_to_pdf: None,
            pending_sync_to_markdown: None,
            last_split_view_size: Vec2::ZERO,
            last_pdf_scroll_offset_y: 0.0,
            last_markdown_scroll_offset_y: 0.0,
            last_markdown_toggle_at: -10.0,
            pending_markdown_exit_doc_id: None,
            pending_markdown_edit_focus_page: None,
            runtime_check,
            runtime_popup_open,
            settings_window_open: false,
            runtime_manifest,
            runtime_assets,
            selected_runtime_asset: 0,
            runtime_install_backends,
            selected_runtime_install_backend,
            runtime_download_in_progress: false,
            runtime_unblock_in_progress: false,
            runtime_post_install_prompt: false,
            model_download_in_progress: false,
            selected_model_combo_preset: 0,
            pending_mmproj_combo_preset: None,
            device_enumeration_in_progress: false,
            available_devices: Vec::new(),
            last_enumerated_runtime_dir: None,
            device_options: initial_device_options,
            selected_device_option: initial_selected_device_option,
            runtime_status,
            background_tx,
            background_rx,
            pending_document_load_results: VecDeque::new(),
        };

        app.sync_runtime_backend_from_installed_runtime();

        if app.runtime_check.is_ok() {
            app.ensure_devices_enumerated_for_runtime();
        }

        app
    }

    fn push_log(&mut self, level: LogLevel, message: impl Into<String>) {
        let entry = LogEntry {
            timestamp: format_log_timestamp_now(),
            level,
            message: message.into(),
        };
        self.log_entries.push(entry);
        const MAX_LOG_ENTRIES: usize = 4000;
        if self.log_entries.len() > MAX_LOG_ENTRIES {
            let overflow = self.log_entries.len() - MAX_LOG_ENTRIES;
            self.log_entries.drain(0..overflow);
        }
    }

    fn next_job_id(&mut self) -> u64 {
        let id = self.next_job_id;
        self.next_job_id += 1;
        id
    }

    fn create_job(
        &mut self,
        kind: JobKind,
        title: impl Into<String>,
        state: JobState,
        detail: impl Into<String>,
    ) -> u64 {
        let id = self.next_job_id();
        self.jobs.push(JobRecord {
            id,
            kind,
            title: title.into(),
            state,
            detail: detail.into(),
            progress_percent: None,
        });
        id
    }

    fn find_job_mut(&mut self, job_id: u64) -> Option<&mut JobRecord> {
        self.jobs.iter_mut().find(|job| job.id == job_id)
    }

    fn update_job_state(
        &mut self,
        job_id: u64,
        state: JobState,
        detail: impl Into<String>,
        progress_percent: Option<f32>,
    ) {
        if let Some(job) = self.find_job_mut(job_id) {
            job.state = state;
            job.detail = detail.into();
            job.progress_percent = progress_percent;
        }
    }

    fn parse_percent_from_status(status: &str) -> Option<f32> {
        for token in status.split_whitespace() {
            let cleaned = token
                .trim_matches(|c: char| c == '(' || c == ')' || c == '[' || c == ']')
                .strip_suffix('%');
            if let Some(number) = cleaned {
                if let Ok(value) = number.parse::<f32>() {
                    if value.is_finite() {
                        return Some(value.clamp(0.0, 100.0));
                    }
                }
            }
        }
        None
    }

    fn job_counts(&self) -> (usize, usize, usize, usize) {
        let mut pending = 0usize;
        let mut running = 0usize;
        let mut completed = 0usize;
        let mut failed = 0usize;
        for job in &self.jobs {
            match job.state {
                JobState::Pending => pending += 1,
                JobState::Running => running += 1,
                JobState::Completed => completed += 1,
                JobState::Failed => failed += 1,
            }
        }
        (pending, running, completed, failed)
    }

    fn active_job_summary(&self) -> String {
        if let Some(job) = self
            .jobs
            .iter()
            .rev()
            .find(|job| job.state == JobState::Running)
        {
            if let Some(percent) = job.progress_percent {
                return format!(
                    "{}: {} ({percent:.1}%)",
                    job.kind.label(),
                    job.detail.trim()
                );
            }
            return format!("{}: {}", job.kind.label(), job.detail.trim());
        }
        if let Some(job) = self
            .jobs
            .iter()
            .rev()
            .find(|job| job.state == JobState::Pending)
        {
            return format!("Queued: {}", job.title);
        }
        "Idle".to_owned()
    }

    fn backend_priority_for_platform(backend_norm: &str) -> i32 {
        #[cfg(target_os = "windows")]
        {
            if backend_norm.contains("cuda") {
                return 500;
            }
            if backend_norm.contains("vulkan") {
                return 400;
            }
            if backend_norm.contains("metal") {
                return 300;
            }
            return 100;
        }

        #[cfg(target_os = "macos")]
        {
            if backend_norm.contains("metal") {
                return 500;
            }
            if backend_norm.contains("vulkan") {
                return 400;
            }
            if backend_norm.contains("cuda") {
                return 200;
            }
            return 100;
        }

        #[cfg(target_os = "linux")]
        {
            if backend_norm.contains("vulkan") {
                return 500;
            }
            if backend_norm.contains("cuda") {
                return 400;
            }
            if backend_norm.contains("metal") {
                return 200;
            }
            return 100;
        }

        #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
        {
            if backend_norm.contains("vulkan") {
                return 500;
            }
            if backend_norm.contains("cuda") {
                return 400;
            }
            if backend_norm.contains("metal") {
                return 300;
            }
            return 100;
        }
    }

    fn format_bytes(bytes: u64) -> String {
        const KIB: f64 = 1024.0;
        const MIB: f64 = KIB * 1024.0;
        const GIB: f64 = MIB * 1024.0;
        let b = bytes as f64;
        if b >= GIB {
            format!("{:.2} GiB", b / GIB)
        } else if b >= MIB {
            format!("{:.2} MiB", b / MIB)
        } else if b >= KIB {
            format!("{:.2} KiB", b / KIB)
        } else {
            format!("{bytes} B")
        }
    }

    fn model_combo_preset(index: usize) -> ModelComboPreset {
        let clamped = index.min(MODEL_COMBO_PRESETS.len().saturating_sub(1));
        MODEL_COMBO_PRESETS[clamped]
    }

    fn current_model_combo_preset(&self) -> ModelComboPreset {
        Self::model_combo_preset(self.selected_model_combo_preset)
    }

    fn default_runtime_backends_for_platform() -> Vec<String> {
        if cfg!(target_os = "windows") {
            return vec!["vulkan".to_owned(), "cuda".to_owned()];
        }
        if cfg!(target_os = "macos") {
            return vec!["metal".to_owned()];
        }
        vec!["vulkan".to_owned()]
    }

    fn runtime_backend_options_from_assets(assets: &[ManifestAsset]) -> Vec<String> {
        let mut options = assets
            .iter()
            .map(|asset| asset.backend.trim().to_ascii_lowercase())
            .filter(|backend| !backend.is_empty())
            .collect::<Vec<_>>();
        options.sort();
        options.dedup();
        if options.is_empty() {
            options = Self::default_runtime_backends_for_platform();
        }
        if cfg!(target_os = "windows") {
            options.sort_by_key(|backend| match backend.as_str() {
                "vulkan" => 0usize,
                "cuda" => 1usize,
                _ => 9usize,
            });
        }
        options
    }

    fn resolve_runtime_backend_index(backends: &[String], configured_backend: &str) -> usize {
        if backends.is_empty() {
            return 0;
        }
        let configured = configured_backend.trim();
        if configured.is_empty() {
            return 0;
        }
        backends
            .iter()
            .position(|backend| backend.eq_ignore_ascii_case(configured))
            .unwrap_or(0)
    }

    fn build_vlm_device_options(devices: &[EngineDevice]) -> Vec<VlmDeviceOption> {
        let mut options = vec![VlmDeviceOption {
            label: VLM_DEVICE_CPU_LABEL.to_owned(),
            devices_value: "none".to_owned(),
            main_gpu: 0,
            is_gpu: false,
            detail_line: VLM_DEVICE_CPU_LABEL.to_owned(),
        }];
        let mut chosen_by_gpu = HashMap::<String, (i32, VlmDeviceOption)>::new();

        for device in devices {
            let backend = if device.backend.trim().is_empty() {
                "unknown".to_owned()
            } else {
                device.backend.trim().to_owned()
            };
            let name = if device.name.trim().is_empty() {
                "unnamed".to_owned()
            } else {
                device.name.trim().to_owned()
            };
            let description = device.description.trim().to_owned();
            let backend_norm = backend.to_ascii_lowercase();
            let name_norm = name.to_ascii_lowercase();
            if backend_norm.contains("cpu") || name_norm == "cpu" {
                continue;
            }

            let gpu_identity = if description.is_empty() {
                name_norm.clone()
            } else {
                description.to_ascii_lowercase()
            };
            let gpu_index = device.index.max(0);
            let label = if !description.is_empty() {
                format!("GPU #{gpu_index} ({backend}): {description}")
            } else {
                format!("GPU #{gpu_index} ({backend}): {name}")
            };
            let detail_line = format!(
                "{} (free {} / total {})",
                label,
                Self::format_bytes(device.memory_free),
                Self::format_bytes(device.memory_total)
            );
            let option = VlmDeviceOption {
                label,
                devices_value: gpu_index.to_string(),
                main_gpu: gpu_index,
                is_gpu: true,
                detail_line,
            };
            let priority = Self::backend_priority_for_platform(&backend_norm);
            match chosen_by_gpu.get(&gpu_identity) {
                Some((current_priority, _)) if *current_priority >= priority => {}
                _ => {
                    chosen_by_gpu.insert(gpu_identity, (priority, option));
                }
            }
        }

        let mut selected = chosen_by_gpu
            .into_values()
            .map(|(_, option)| option)
            .collect::<Vec<_>>();
        selected.sort_by_key(|option| option.main_gpu);
        options.extend(selected);

        if options.len() == 1 {
            options[0].detail_line =
                "CPU (no GPU) - no compatible GPU backend detected, using CPU mode.".to_owned();
        }

        options
    }

    #[cfg(target_os = "macos")]
    fn preferred_gpu_option_index(options: &[VlmDeviceOption]) -> Option<usize> {
        let mut first_gpu: Option<usize> = None;
        for (index, option) in options.iter().enumerate() {
            if !option.is_gpu {
                continue;
            }
            if first_gpu.is_none() {
                first_gpu = Some(index);
            }
            let label = option.label.to_ascii_lowercase();
            let detail = option.detail_line.to_ascii_lowercase();
            if label.contains("metal") || detail.contains("metal") {
                return Some(index);
            }
        }
        first_gpu
    }

    fn resolve_selected_device_index(options: &[VlmDeviceOption], settings: &AppSettings) -> usize {
        if options.is_empty() {
            return 0;
        }

        let cpu_index = options
            .iter()
            .position(|option| !option.is_gpu)
            .unwrap_or(0);
        let configured_gpu = parse_gpu_index_setting(settings.devices.trim());
        if let Some(gpu_index) = configured_gpu {
            let configured_gpu_value = gpu_index.to_string();
            if let Some(index) = options
                .iter()
                .position(|option| option.is_gpu && option.devices_value == configured_gpu_value)
            {
                return index;
            }
            #[cfg(target_os = "macos")]
            if let Some(gpu_index) = Self::preferred_gpu_option_index(options) {
                return gpu_index;
            }
            return cpu_index;
        }

        #[cfg(target_os = "macos")]
        if let Some(gpu_index) = Self::preferred_gpu_option_index(options) {
            return gpu_index;
        }
        cpu_index
    }

    fn rebuild_device_options_from_available(&mut self) {
        self.device_options = Self::build_vlm_device_options(&self.available_devices);
        self.selected_device_option =
            Self::resolve_selected_device_index(&self.device_options, &self.settings)
                .min(self.device_options.len().saturating_sub(1));
    }

    fn apply_selected_device_to_settings(&mut self) {
        #[cfg(target_os = "macos")]
        let Some(mut option) = self
            .device_options
            .get(self.selected_device_option)
            .cloned()
        else {
            return;
        };
        #[cfg(not(target_os = "macos"))]
        let Some(option) = self
            .device_options
            .get(self.selected_device_option)
            .cloned()
        else {
            return;
        };

        #[cfg(target_os = "macos")]
        if !option.is_gpu {
            if let Some(gpu_index) = Self::preferred_gpu_option_index(&self.device_options) {
                self.selected_device_option = gpu_index;
                if let Some(forced_option) = self.device_options.get(gpu_index).cloned() {
                    option = forced_option;
                }
            }
        }

        if option.is_gpu {
            self.settings.devices = option.devices_value;
            self.settings.n_threads = 0;
            self.settings.n_threads_batch = 0;
        } else {
            self.settings.devices = "none".to_owned();
            if self.settings.n_threads < 0 {
                self.settings.n_threads = 0;
            }
            if self.settings.n_threads_batch < 0 {
                self.settings.n_threads_batch = 0;
            }
        }
    }

    fn has_background_work(&self) -> bool {
        self.jobs
            .iter()
            .any(|job| matches!(job.state, JobState::Pending | JobState::Running))
    }

    #[cfg(target_os = "linux")]
    fn has_non_conversion_background_work(&self) -> bool {
        self.jobs.iter().any(|job| {
            matches!(job.state, JobState::Pending | JobState::Running)
                && !matches!(job.kind, JobKind::Conversion)
        })
    }

    fn background_repaint_interval(&self) -> Duration {
        #[cfg(target_os = "linux")]
        {
            if !self.has_non_conversion_background_work() {
                return Duration::from_millis(CONVERSION_ONLY_REPAINT_MS_LINUX);
            }
        }
        Duration::from_millis(BACKGROUND_REPAINT_MS)
    }

    fn vlm_model_ready(&self) -> bool {
        configured_path_exists(&self.settings.vlm_model_path)
    }

    fn vlm_mmproj_ready(&self) -> bool {
        configured_path_exists(&self.settings.vlm_mmproj_path)
    }

    fn vlm_stack_ready(&self) -> bool {
        self.vlm_model_ready() && self.vlm_mmproj_ready()
    }

    fn has_missing_essentials(&self) -> bool {
        !self.runtime_check.is_ok() || !self.vlm_stack_ready()
    }

    fn open_setup_modal(&mut self, message: impl Into<String>) {
        self.status_message = message.into();
        self.runtime_popup_open = true;
        self.settings_window_open = true;
        self.push_log(LogLevel::Warn, self.status_message.clone());
    }

    fn set_legal_doc_kind(&mut self, kind: LegalDocKind) {
        self.legal_doc_kind = kind;
        self.legal_doc_lines = kind
            .bundled_markdown()
            .lines()
            .map(|line| line.to_owned())
            .collect();
    }

    fn open_legal_doc(&mut self, kind: LegalDocKind) {
        self.set_legal_doc_kind(kind);
        self.legal_docs_window_open = true;
    }

    fn update_setup_modal_after_requirement_change(&mut self) {
        if !self.has_missing_essentials() {
            self.runtime_popup_open = false;
        }
    }

    fn reset_device_enumeration_cache(&mut self) {
        self.available_devices.clear();
        self.last_enumerated_runtime_dir = None;
        self.rebuild_device_options_from_available();
    }

    fn ensure_devices_enumerated_for_runtime(&mut self) {
        if self.device_enumeration_in_progress {
            return;
        }
        self.refresh_runtime_state();
        if !self.runtime_check.is_ok() {
            return;
        }

        let runtime_dir = self.effective_runtime_dir();
        let need_enumeration = self
            .last_enumerated_runtime_dir
            .as_ref()
            .map_or(true, |last| last != &runtime_dir);
        if need_enumeration {
            self.start_device_enumeration();
        }
    }

    fn conversion_mode_label(mode: &ConversionMode) -> &'static str {
        match mode {
            ConversionMode::FastPdf => "FAST",
            ConversionMode::PdfVlm => "VLM",
            ConversionMode::FastPdfWithVlmFallback => "FAST->VLM fallback",
        }
    }

    fn vlm_runtime_summary(request: &PdfVlmRequest) -> String {
        format!(
            "gpu={} n_ctx={} batch={} ubatch={} parallel={} threads={} threads_batch={}",
            request
                .gpu
                .map(|gpu| gpu.max(0).to_string())
                .unwrap_or_else(|| "cpu".to_owned()),
            request.n_ctx,
            request.n_batch,
            request.n_ubatch,
            request.n_parallel,
            request.n_threads,
            request.n_threads_batch
        )
    }

    fn resolved_pdf_vlm_prompt(&self) -> String {
        let prompt = self.settings.vlm_prompt.trim();
        if prompt.is_empty() {
            DEFAULT_VLM_PROMPT.to_owned()
        } else {
            prompt.to_owned()
        }
    }

    fn resolved_image_vlm_prompt(&self) -> String {
        let prompt = self.settings.vlm_image_prompt.trim();
        if prompt.is_empty() {
            DEFAULT_IMAGE_VLM_PROMPT.to_owned()
        } else {
            prompt.to_owned()
        }
    }

    fn fast_error_looks_non_machine_readable(error: &str) -> bool {
        let normalized = error.to_ascii_lowercase();
        normalized.contains(FAST_MACHINE_READABILITY_HINT_A)
            || normalized.contains(FAST_MACHINE_READABILITY_HINT_B)
            || normalized.contains("broken/missing unicode font mapping")
            || normalized.contains("ocr fallback")
    }

    fn read_markdown_output(path: &Path) -> Result<String, String> {
        std::fs::read_to_string(path).map_err(|err| {
            format!(
                "failed reading converter output '{}': {err}",
                path.display()
            )
        })
    }

    fn normalized_markdown_output(path: &Path) -> Result<String, String> {
        let raw = Self::read_markdown_output(path)?;
        let sanitized = sanitize_outer_markdown_fence(&raw);
        if sanitized != raw {
            std::fs::write(path, sanitized.as_bytes()).map_err(|err| {
                format!(
                    "failed writing sanitized markdown output '{}': {err}",
                    path.display()
                )
            })?;
        }
        Ok(sanitized)
    }

    fn build_vlm_request_for_document(
        &self,
        doc_path: &Path,
        runtime_dir: &Path,
        output_path: PathBuf,
    ) -> Result<PdfVlmRequest, String> {
        if self.settings.vlm_model_path.trim().is_empty() {
            return Err("VLM model path is empty".to_owned());
        }
        if self.settings.vlm_mmproj_path.trim().is_empty() {
            return Err("VLM mmproj path is empty".to_owned());
        }

        let is_image = !app_config::is_pdf(doc_path);
        let prompt = if is_image {
            self.resolved_image_vlm_prompt()
        } else {
            self.resolved_pdf_vlm_prompt()
        };

        let resolved_device_index =
            Self::resolve_selected_device_index(&self.device_options, &self.settings)
                .min(self.device_options.len().saturating_sub(1));
        let selected_device = self.device_options.get(resolved_device_index);
        let configured_gpu = parse_gpu_index_setting(self.settings.devices.trim());
        let configured_gpu_available = configured_gpu.and_then(|gpu| {
            let gpu_value = gpu.to_string();
            self.device_options
                .iter()
                .any(|option| option.is_gpu && option.devices_value == gpu_value)
                .then_some(gpu)
        });
        let runtime_enumeration_is_fresh = self
            .last_enumerated_runtime_dir
            .as_ref()
            .is_some_and(|last| last == runtime_dir);
        let has_known_gpu_option = self.device_options.iter().any(|option| option.is_gpu);
        let resolved_gpu = if has_known_gpu_option {
            configured_gpu_available.or_else(|| match selected_device {
                Some(option) if option.is_gpu => Some(option.main_gpu.max(0)),
                _ => None,
            })
        } else if runtime_enumeration_is_fresh {
            None
        } else {
            configured_gpu
        };
        #[cfg(target_os = "macos")]
        let resolved_gpu = resolved_gpu.or_else(|| {
            Self::preferred_gpu_option_index(&self.device_options)
                .and_then(|index| self.device_options.get(index))
                .map(|option| option.main_gpu.max(0))
        });
        #[cfg(target_os = "macos")]
        if resolved_gpu.is_none() {
            return Err(
                "No GPU execution device available on macOS. VLM CPU mode is disabled for macOS arm builds; install/repair runtime and enumerate devices."
                    .to_owned(),
            );
        }

        Ok(PdfVlmRequest {
            input_path: doc_path.to_path_buf(),
            is_image,
            model_path: PathBuf::from(self.settings.vlm_model_path.trim()),
            mmproj_path: PathBuf::from(self.settings.vlm_mmproj_path.trim()),
            output_md_path: output_path,
            pdfium_lib_path: runtime_pdfium_library_path(runtime_dir),
            prompt,
            n_predict: self.settings.n_predict,
            n_ctx: self.settings.n_ctx,
            n_batch: self.settings.n_batch,
            n_ubatch: self.settings.n_ubatch,
            n_parallel: self.settings.n_parallel,
            n_threads: self.settings.n_threads,
            n_threads_batch: self.settings.n_threads_batch,
            gpu: resolved_gpu,
        })
    }

    fn run_conversion_task(
        task: &ConversionTask,
        mut on_status: impl FnMut(String),
    ) -> Result<ConversionTaskOutcome, String> {
        match &task.payload {
            ConversionTaskPayload::FastPdf {
                input_path,
                output_path,
            } => {
                on_status("Running FAST extraction...".to_owned());
                run_pdf_fast(&task.runtime_dir, input_path, output_path)?;
                let markdown = Self::normalized_markdown_output(output_path)?;
                Ok(ConversionTaskOutcome {
                    markdown,
                    output_path: output_path.clone(),
                    used_mode: ConversionMode::FastPdf,
                    fallback_used: false,
                    fast_error: None,
                })
            }
            ConversionTaskPayload::PdfVlm { request } => {
                on_status("Running VLM extraction...".to_owned());
                if request.is_image {
                    run_image_vlm(&task.runtime_dir, request)?;
                } else {
                    run_pdf_vlm(&task.runtime_dir, request)?;
                }
                let markdown = Self::normalized_markdown_output(&request.output_md_path)?;
                Ok(ConversionTaskOutcome {
                    markdown,
                    output_path: request.output_md_path.clone(),
                    used_mode: ConversionMode::PdfVlm,
                    fallback_used: false,
                    fast_error: None,
                })
            }
            ConversionTaskPayload::FastPdfWithVlmFallback {
                input_path,
                fast_output_path,
                fallback_request,
            } => {
                on_status("Running FAST extraction (VLM fallback enabled)...".to_owned());
                match run_pdf_fast(&task.runtime_dir, input_path, fast_output_path) {
                    Ok(()) => {
                        let markdown = Self::normalized_markdown_output(fast_output_path)?;
                        Ok(ConversionTaskOutcome {
                            markdown,
                            output_path: fast_output_path.clone(),
                            used_mode: ConversionMode::FastPdf,
                            fallback_used: false,
                            fast_error: None,
                        })
                    }
                    Err(fast_error) => {
                        if !Self::fast_error_looks_non_machine_readable(&fast_error) {
                            return Err(format!("FAST extraction failed: {fast_error}"));
                        }

                        on_status(format!(
                            "FAST extraction rejected as non-machine-readable; running VLM fallback. ({})",
                            fast_error
                        ));
                        run_pdf_vlm(&task.runtime_dir, fallback_request)?;
                        let markdown =
                            Self::normalized_markdown_output(&fallback_request.output_md_path)?;
                        Ok(ConversionTaskOutcome {
                            markdown,
                            output_path: fallback_request.output_md_path.clone(),
                            used_mode: ConversionMode::PdfVlm,
                            fallback_used: true,
                            fast_error: Some(fast_error),
                        })
                    }
                }
            }
        }
    }

    fn maybe_start_next_conversion(&mut self) {
        if self.active_conversion_job.is_some() {
            return;
        }
        let Some(task) = self.conversion_queue.pop_front() else {
            return;
        };

        self.active_conversion_job = Some(task.job_id);
        self.active_conversion_key = Some((task.doc_id, task.mode.clone()));
        let mode_label = Self::conversion_mode_label(&task.mode);
        self.update_job_state(
            task.job_id,
            JobState::Running,
            format!("Converting {} ({mode_label})...", task.doc_name),
            None,
        );
        self.push_log(
            LogLevel::Info,
            format!("Started {} conversion: {}", mode_label, task.doc_name),
        );
        match &task.payload {
            ConversionTaskPayload::PdfVlm { request } => {
                self.push_log(
                    LogLevel::Info,
                    format!(
                        "VLM runtime params for '{}': {}",
                        task.doc_name,
                        Self::vlm_runtime_summary(request)
                    ),
                );
            }
            ConversionTaskPayload::FastPdfWithVlmFallback {
                fallback_request, ..
            } => {
                self.push_log(
                    LogLevel::Info,
                    format!(
                        "Fallback VLM params for '{}': {}",
                        task.doc_name,
                        Self::vlm_runtime_summary(fallback_request)
                    ),
                );
            }
            ConversionTaskPayload::FastPdf { .. } => {}
        }

        let tx = self.background_tx.clone();
        thread::spawn(move || {
            let progress_tx = tx.clone();
            let result = Self::run_conversion_task(&task, |status| {
                let _ = progress_tx.send(BackgroundEvent::JobProgress {
                    job_id: task.job_id,
                    status,
                });
            });
            let _ = tx.send(BackgroundEvent::ConversionFinished {
                job_id: task.job_id,
                doc_id: task.doc_id,
                doc_name: task.doc_name,
                requested_mode: task.mode,
                result,
            });
        });
    }

    fn queue_checked_conversions(&mut self) {
        if self.documents.is_empty() {
            self.status_message = "No loaded documents to queue.".to_owned();
            return;
        }

        self.refresh_runtime_state();
        if !self.runtime_check.is_ok() {
            let message = format!(
                "Runtime is missing required files: {}",
                self.runtime_check.missing.join(" | ")
            );
            self.open_setup_modal(message);
            return;
        }

        let mut indices = self
            .documents
            .iter()
            .enumerate()
            .filter_map(|(index, document)| document.selected_for_conversion.then_some(index))
            .collect::<Vec<_>>();
        if indices.is_empty() {
            if let Some(index) = self.selected_doc_index() {
                indices.push(index);
            } else {
                self.status_message =
                    "No checked documents. Tick files in the Documents list to convert.".to_owned();
                return;
            }
        }

        let mut queued = 0usize;
        let mut skipped = 0usize;
        for index in indices {
            match self.queue_conversion_for_index(index) {
                Ok(_) => queued += 1,
                Err(_) => skipped += 1,
            }
        }
        self.status_message = if skipped > 0 {
            format!("Queued {queued} conversion(s), skipped {skipped}.")
        } else {
            format!("Queued {queued} conversion(s).")
        };
    }

    fn queue_conversion_for_index(&mut self, doc_index: usize) -> Result<&'static str, String> {
        self.refresh_runtime_state();
        if !self.runtime_check.is_ok() {
            let message = format!(
                "Runtime is missing required files: {}",
                self.runtime_check.missing.join(" | ")
            );
            self.open_setup_modal(message.clone());
            return Err(message);
        }

        let Some(document) = self.documents.get(doc_index) else {
            return Err("document index out of bounds".to_owned());
        };
        let doc_id = document.id;
        let doc_name = document.name.clone();
        let doc_path = document.path.clone();
        let doc_is_pdf = app_config::is_pdf(&doc_path);

        let mode = if doc_is_pdf {
            self.settings.conversion_mode.clone()
        } else {
            ConversionMode::PdfVlm
        };
        let needs_vlm = !doc_is_pdf
            || matches!(
                mode,
                ConversionMode::PdfVlm | ConversionMode::FastPdfWithVlmFallback
            );
        if needs_vlm && !self.vlm_stack_ready() {
            let message = if doc_is_pdf {
                "VLM model and MMProj are required for PDF VLM / FAST fallback mode. Open settings and download/set both model files."
                    .to_owned()
            } else {
                "Images always require VLM model + MMProj. Open settings and download/set both model files."
                    .to_owned()
            };
            self.open_setup_modal(message.clone());
            return Err(message);
        }

        let mode_label = Self::conversion_mode_label(&mode);
        let runtime_dir = self.effective_runtime_dir();
        let fast_output_path = Self::conversion_output_path_for_source(&doc_path, "FAST")?;
        let vlm_output_path = Self::conversion_output_path_for_source(&doc_path, "VLM")?;

        if self
            .conversion_queue
            .iter()
            .any(|task| task.doc_id == doc_id && task.mode == mode)
            || self
                .active_conversion_key
                .as_ref()
                .is_some_and(|(active_doc_id, active_mode)| {
                    *active_doc_id == doc_id && active_mode == &mode
                })
        {
            return Err("document is already queued for this mode".to_owned());
        }

        let payload = match mode {
            ConversionMode::FastPdf => ConversionTaskPayload::FastPdf {
                input_path: doc_path.clone(),
                output_path: fast_output_path,
            },
            ConversionMode::PdfVlm => {
                let request =
                    self.build_vlm_request_for_document(&doc_path, &runtime_dir, vlm_output_path)?;
                ConversionTaskPayload::PdfVlm { request }
            }
            ConversionMode::FastPdfWithVlmFallback => {
                if !doc_is_pdf {
                    let request = self.build_vlm_request_for_document(
                        &doc_path,
                        &runtime_dir,
                        vlm_output_path,
                    )?;
                    ConversionTaskPayload::PdfVlm { request }
                } else {
                    let fallback_request = self.build_vlm_request_for_document(
                        &doc_path,
                        &runtime_dir,
                        vlm_output_path,
                    )?;
                    ConversionTaskPayload::FastPdfWithVlmFallback {
                        input_path: doc_path.clone(),
                        fast_output_path,
                        fallback_request,
                    }
                }
            }
        };

        let job_id = self.create_job(
            JobKind::Conversion,
            format!("{doc_name} ({mode_label})"),
            JobState::Pending,
            "Queued for conversion",
        );

        self.conversion_queue.push_back(ConversionTask {
            job_id,
            doc_id,
            doc_name: doc_name.clone(),
            mode,
            runtime_dir,
            payload,
        });
        self.push_log(
            LogLevel::Info,
            format!("Queued {} conversion: {}", mode_label, doc_name),
        );
        self.maybe_start_next_conversion();
        Ok(mode_label)
    }

    fn effective_runtime_dir(&self) -> PathBuf {
        runtime_dir_from_settings(&self.settings, &self.paths)
    }

    fn sync_runtime_backend_from_installed_runtime(&mut self) {
        #[cfg(target_os = "windows")]
        {
            if !self.runtime_check.is_ok() {
                return;
            }
            let runtime_dir = self.effective_runtime_dir();
            let Some(detected_backend) = detect_installed_runtime_backend(&runtime_dir) else {
                return;
            };

            if self.runtime_install_backends.is_empty() {
                self.runtime_install_backends = Self::default_runtime_backends_for_platform();
            }
            self.selected_runtime_install_backend = Self::resolve_runtime_backend_index(
                &self.runtime_install_backends,
                &detected_backend,
            )
            .min(self.runtime_install_backends.len().saturating_sub(1));
            if let Some(backend) = self
                .runtime_install_backends
                .get(self.selected_runtime_install_backend)
            {
                self.settings.runtime_download_backend = backend.clone();
            } else {
                self.settings.runtime_download_backend = detected_backend;
            }
        }
    }

    fn refresh_runtime_state(&mut self) {
        self.runtime_check = check_runtime_dir(&self.effective_runtime_dir());
        self.sync_runtime_backend_from_installed_runtime();
    }

    fn reload_runtime_manifest(&mut self) {
        match load_engine_manifest(&self.paths) {
            Ok(manifest) => {
                self.runtime_assets = filtered_assets_for_platform(&manifest);
                self.runtime_install_backends =
                    Self::runtime_backend_options_from_assets(&self.runtime_assets);
                self.selected_runtime_install_backend = Self::resolve_runtime_backend_index(
                    &self.runtime_install_backends,
                    &self.settings.runtime_download_backend,
                )
                .min(self.runtime_install_backends.len().saturating_sub(1));
                if let Some(backend) = self
                    .runtime_install_backends
                    .get(self.selected_runtime_install_backend)
                {
                    self.settings.runtime_download_backend = backend.clone();
                }
                let tag = if manifest.tag.trim().is_empty() {
                    "latest".to_owned()
                } else {
                    manifest.tag.clone()
                };
                self.runtime_manifest = Some(manifest);
                self.runtime_status = format!("Runtime manifest loaded ({tag}).");
            }
            Err(err) => {
                self.runtime_manifest = None;
                self.runtime_assets.clear();
                self.runtime_install_backends = Self::default_runtime_backends_for_platform();
                self.selected_runtime_install_backend = Self::resolve_runtime_backend_index(
                    &self.runtime_install_backends,
                    &self.settings.runtime_download_backend,
                )
                .min(self.runtime_install_backends.len().saturating_sub(1));
                if let Some(backend) = self
                    .runtime_install_backends
                    .get(self.selected_runtime_install_backend)
                {
                    self.settings.runtime_download_backend = backend.clone();
                }
                self.runtime_status = format!("Failed to load runtime manifest: {err}");
            }
        }
        if self.selected_runtime_asset >= self.runtime_assets.len() {
            self.selected_runtime_asset = 0;
        }
        self.sync_runtime_backend_from_installed_runtime();
    }

    fn process_background_events(&mut self, ctx: &egui::Context) {
        let mut processed_events = 0usize;
        while processed_events < MAX_BACKGROUND_EVENTS_PER_FRAME {
            let Ok(event) = self.background_rx.try_recv() else {
                break;
            };
            processed_events += 1;
            match event {
                BackgroundEvent::JobProgress { job_id, status } => {
                    let percent = Self::parse_percent_from_status(&status);
                    self.update_job_state(job_id, JobState::Running, status.clone(), percent);
                    self.runtime_status = status;
                }
                BackgroundEvent::RuntimeInstalled { job_id, result } => {
                    self.runtime_download_in_progress = false;
                    match result {
                        Ok(()) => {
                            self.runtime_post_install_prompt =
                                runtime_unblock_required_for_platform();
                            self.runtime_status = if runtime_unblock_required_for_platform() {
                                "Runtime install complete. Run 'Unblock unsigned runtime', then reload runtime check.".to_owned()
                            } else {
                                "Runtime install complete.".to_owned()
                            };
                            self.status_message = self.runtime_status.clone();
                            self.refresh_runtime_state();
                            self.reset_device_enumeration_cache();
                            self.ensure_devices_enumerated_for_runtime();
                            self.runtime_popup_open = true;
                            self.settings_window_open = true;
                            self.update_job_state(
                                job_id,
                                JobState::Completed,
                                "Runtime install completed",
                                Some(100.0),
                            );
                            self.push_log(LogLevel::Info, self.runtime_status.clone());
                        }
                        Err(err) => {
                            self.runtime_post_install_prompt = false;
                            self.runtime_status = format!("Runtime install failed: {err}");
                            self.open_setup_modal(self.runtime_status.clone());
                            self.update_job_state(
                                job_id,
                                JobState::Failed,
                                self.runtime_status.clone(),
                                None,
                            );
                            self.push_log(LogLevel::Error, self.runtime_status.clone());
                        }
                    }
                }
                BackgroundEvent::RuntimeUnblocked { job_id, result } => {
                    self.runtime_unblock_in_progress = false;
                    match result {
                        Ok(message) => {
                            self.runtime_post_install_prompt = false;
                            self.runtime_status = message;
                            self.status_message = self.runtime_status.clone();
                            self.refresh_runtime_state();
                            self.reset_device_enumeration_cache();
                            self.ensure_devices_enumerated_for_runtime();
                            self.update_setup_modal_after_requirement_change();
                            self.update_job_state(
                                job_id,
                                JobState::Completed,
                                "Unsigned runtime unblock completed",
                                Some(100.0),
                            );
                            self.push_log(LogLevel::Info, self.runtime_status.clone());
                        }
                        Err(err) => {
                            self.runtime_status = format!("Unsigned runtime unblock failed: {err}");
                            self.status_message = self.runtime_status.clone();
                            self.runtime_popup_open = true;
                            self.settings_window_open = true;
                            self.update_job_state(
                                job_id,
                                JobState::Failed,
                                self.runtime_status.clone(),
                                None,
                            );
                            self.push_log(LogLevel::Error, self.runtime_status.clone());
                        }
                    }
                }
                BackgroundEvent::DevicesEnumerated { job_id, result } => {
                    self.device_enumeration_in_progress = false;
                    match result {
                        Ok(devices) => {
                            self.available_devices = devices;
                            self.rebuild_device_options_from_available();
                            self.runtime_status = format!(
                                "Enumerated {} runtime device(s); {} execution option(s) available.",
                                self.available_devices.len(),
                                self.device_options.len()
                            );
                            self.update_job_state(
                                job_id,
                                JobState::Completed,
                                self.runtime_status.clone(),
                                Some(100.0),
                            );
                            self.push_log(LogLevel::Info, self.runtime_status.clone());
                        }
                        Err(err) => {
                            self.runtime_status = format!("Device enumeration failed: {err}");
                            self.available_devices.clear();
                            self.rebuild_device_options_from_available();
                            self.update_job_state(
                                job_id,
                                JobState::Failed,
                                self.runtime_status.clone(),
                                None,
                            );
                            self.push_log(LogLevel::Error, self.runtime_status.clone());
                        }
                    }
                }
                BackgroundEvent::ModelDownloaded {
                    job_id,
                    purpose,
                    result,
                } => {
                    self.model_download_in_progress = false;
                    match result {
                        Ok(path) => {
                            self.settings.model_download_file_name = path
                                .file_name()
                                .and_then(|name| name.to_str())
                                .unwrap_or_default()
                                .to_owned();
                            let status = match purpose {
                                ModelDownloadPurpose::Model => {
                                    self.settings.vlm_model_path = path.display().to_string();
                                    format!(
                                        "VLM model downloaded and set as default: '{}'.",
                                        path.display()
                                    )
                                }
                                ModelDownloadPurpose::Mmproj => {
                                    self.settings.vlm_mmproj_path = path.display().to_string();
                                    format!(
                                        "MMProj downloaded and set as default: '{}'.",
                                        path.display()
                                    )
                                }
                            };
                            self.runtime_status = status;
                            self.status_message = self.runtime_status.clone();
                            if let Err(err) = self.save_settings_now() {
                                self.push_log(
                                    LogLevel::Error,
                                    format!("Failed to save settings after model download: {err}"),
                                );
                            }
                            self.update_setup_modal_after_requirement_change();
                            self.update_job_state(
                                job_id,
                                JobState::Completed,
                                self.runtime_status.clone(),
                                Some(100.0),
                            );
                            self.push_log(LogLevel::Info, self.runtime_status.clone());

                            if matches!(purpose, ModelDownloadPurpose::Model) {
                                if let Some(pending_combo_index) =
                                    self.pending_mmproj_combo_preset.take()
                                {
                                    let preset = Self::model_combo_preset(pending_combo_index);
                                    let mmproj_destination =
                                        self.paths.models_dir.join(preset.mmproj_file);
                                    self.runtime_status = format!(
                                        "Model downloaded. Starting MMProj download to '{}'.",
                                        mmproj_destination.display()
                                    );
                                    self.status_message = self.runtime_status.clone();
                                    self.push_log(LogLevel::Info, self.runtime_status.clone());
                                    self.start_model_download_with(
                                        preset.mmproj_url.to_owned(),
                                        preset.mmproj_file.to_owned(),
                                        ModelDownloadPurpose::Mmproj,
                                    );
                                }
                            } else if matches!(purpose, ModelDownloadPurpose::Mmproj) {
                                self.runtime_status = format!(
                                    "Model combo downloaded to '{}'.",
                                    self.paths.models_dir.display()
                                );
                                self.status_message = self.runtime_status.clone();
                                self.push_log(LogLevel::Info, self.runtime_status.clone());
                            }
                        }
                        Err(err) => {
                            self.runtime_status = format!("Model download failed: {err}");
                            self.status_message = self.runtime_status.clone();
                            self.update_job_state(
                                job_id,
                                JobState::Failed,
                                self.runtime_status.clone(),
                                None,
                            );
                            self.push_log(LogLevel::Error, self.runtime_status.clone());
                            if matches!(purpose, ModelDownloadPurpose::Model) {
                                self.pending_mmproj_combo_preset = None;
                            }
                        }
                    }
                }
                BackgroundEvent::DocumentLoaded { job_id, result } => {
                    self.pending_document_load_results
                        .push_back((job_id, result));
                }
                BackgroundEvent::ConversionFinished {
                    job_id,
                    doc_id,
                    doc_name,
                    requested_mode,
                    result,
                } => {
                    self.active_conversion_job = None;
                    self.active_conversion_key = None;
                    let mode_label = Self::conversion_mode_label(&requested_mode);
                    match result {
                        Ok(outcome) => {
                            let actual_mode_label = Self::conversion_mode_label(&outcome.used_mode);
                            if let Some(index) =
                                self.documents.iter().position(|doc| doc.id == doc_id)
                            {
                                if let Some(document) = self.documents.get_mut(index) {
                                    document.markdown = outcome.markdown.clone();
                                    document.markdown_edit_mode = false;
                                    document.markdown_edit_baseline = None;
                                    match outcome.used_mode {
                                        ConversionMode::FastPdf => {
                                            document.fast_markdown_path =
                                                Some(outcome.output_path.clone());
                                            document.active_markdown_preview =
                                                MarkdownPreviewKind::Fast;
                                        }
                                        ConversionMode::PdfVlm
                                        | ConversionMode::FastPdfWithVlmFallback => {
                                            document.vlm_markdown_path =
                                                Some(outcome.output_path.clone());
                                            document.active_markdown_preview =
                                                MarkdownPreviewKind::Vlm;
                                        }
                                    }
                                }
                                self.markdown_cache.clear_scrollable();
                                self.search_hits.clear();
                                self.active_search_hit = 0;
                                if outcome.fallback_used {
                                    let fast_reason = outcome.fast_error.as_deref().unwrap_or("");
                                    self.status_message = format!(
                                        "FAST extraction fallback triggered for '{doc_name}'. Switched to VLM and saved: {}. {}",
                                        outcome.output_path.display(),
                                        fast_reason
                                    );
                                } else {
                                    self.status_message = format!(
                                        "{actual_mode_label} conversion completed. Saved next to source: {}",
                                        outcome.output_path.display()
                                    );
                                }
                            } else {
                                self.status_message = format!(
                                    "{mode_label} conversion finished for '{doc_name}', but document was removed from the workspace."
                                );
                            }
                            self.update_job_state(
                                job_id,
                                JobState::Completed,
                                format!(
                                    "{} finished: {}",
                                    actual_mode_label,
                                    outcome.output_path.display()
                                ),
                                Some(100.0),
                            );
                            if outcome.fallback_used {
                                self.push_log(
                                    LogLevel::Warn,
                                    format!(
                                        "FAST extraction rejected for '{}'. Used VLM fallback and saved '{}'.",
                                        doc_name,
                                        outcome.output_path.display()
                                    ),
                                );
                            } else {
                                self.push_log(
                                    LogLevel::Info,
                                    format!(
                                        "{} conversion completed for '{}': {}",
                                        actual_mode_label,
                                        doc_name,
                                        outcome.output_path.display()
                                    ),
                                );
                            }
                        }
                        Err(err) => {
                            if matches!(requested_mode, ConversionMode::FastPdf)
                                && Self::fast_error_looks_non_machine_readable(&err)
                            {
                                self.status_message =
                                    format!("FAST extraction rejected '{}': {}", doc_name, err);
                            } else {
                                self.status_message = format!(
                                    "{mode_label} conversion failed for '{doc_name}': {err}"
                                );
                            }
                            self.update_job_state(
                                job_id,
                                JobState::Failed,
                                self.status_message.clone(),
                                None,
                            );
                            self.push_log(LogLevel::Error, self.status_message.clone());
                        }
                    }

                    self.maybe_start_next_conversion();
                }
            }
        }

        if processed_events > 0 {
            ctx.request_repaint();
        }

        for _ in 0..MAX_DOCUMENT_MATERIALIZATIONS_PER_FRAME {
            let Some((job_id, result)) = self.pending_document_load_results.pop_front() else {
                break;
            };

            match result {
                Ok(payload) => match self.materialize_loaded_document(ctx, payload) {
                    Ok(name) => {
                        let detail = format!("Loaded '{}'", name);
                        self.update_job_state(
                            job_id,
                            JobState::Completed,
                            detail.clone(),
                            Some(100.0),
                        );
                        self.status_message = detail.clone();
                        self.push_log(LogLevel::Info, detail);
                    }
                    Err(err) => {
                        let detail = format!("Failed to materialize loaded document: {err}");
                        self.update_job_state(job_id, JobState::Failed, detail.clone(), None);
                        self.status_message = detail.clone();
                        self.push_log(LogLevel::Error, detail);
                    }
                },
                Err(err) => {
                    let detail = format!("Document load failed: {err}");
                    self.update_job_state(job_id, JobState::Failed, detail.clone(), None);
                    self.status_message = detail.clone();
                    self.push_log(LogLevel::Error, detail);
                }
            }
        }

        if !self.pending_document_load_results.is_empty() {
            if self.pending_document_load_results.len() > 1 {
                self.status_message = format!(
                    "Importing documents... {} remaining",
                    self.pending_document_load_results.len()
                );
            }
            ctx.request_repaint_after(Duration::from_millis(16));
        } else if processed_events == MAX_BACKGROUND_EVENTS_PER_FRAME {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
    }

    fn start_runtime_download(&mut self) {
        if self.runtime_maintenance_in_progress() {
            return;
        }
        if self.runtime_assets.is_empty() {
            self.reload_runtime_manifest();
            if self.runtime_assets.is_empty() {
                self.runtime_status = "No runtime assets available for this platform.".to_owned();
                self.push_log(LogLevel::Warn, self.runtime_status.clone());
                return;
            }
        }

        let selected_backend = self
            .runtime_install_backends
            .get(
                self.selected_runtime_install_backend
                    .min(self.runtime_install_backends.len().saturating_sub(1)),
            )
            .cloned()
            .unwrap_or_default();

        #[cfg(target_os = "windows")]
        let asset = if selected_backend.trim().is_empty() {
            let index = self
                .selected_runtime_asset
                .min(self.runtime_assets.len().saturating_sub(1));
            self.runtime_assets[index].clone()
        } else if let Some(found) = self.runtime_assets.iter().find(|asset| {
            asset
                .backend
                .trim()
                .eq_ignore_ascii_case(selected_backend.trim())
        }) {
            found.clone()
        } else {
            let available = self
                .runtime_assets
                .iter()
                .map(|asset| asset.backend.trim().to_ascii_lowercase())
                .filter(|value| !value.is_empty())
                .collect::<Vec<_>>()
                .join(", ");
            self.runtime_status = format!(
                "Selected runtime backend '{}' is not available in manifest for this platform (available: {}).",
                selected_backend,
                if available.trim().is_empty() {
                    "<none>"
                } else {
                    &available
                }
            );
            self.push_log(LogLevel::Warn, self.runtime_status.clone());
            return;
        };

        #[cfg(not(target_os = "windows"))]
        let asset = {
            let index = self
                .selected_runtime_asset
                .min(self.runtime_assets.len().saturating_sub(1));
            self.runtime_assets[index].clone()
        };
        let runtime_dir = self.effective_runtime_dir();
        let tx = self.background_tx.clone();
        let mode_name = if asset.id.trim().is_empty() {
            asset.file_name.clone()
        } else {
            asset.id.clone()
        };
        let job_id = self.create_job(
            JobKind::RuntimeInstall,
            format!("Install runtime ({mode_name})"),
            JobState::Running,
            "Preparing runtime installation...",
        );

        self.runtime_download_in_progress = true;
        self.runtime_post_install_prompt = false;
        self.runtime_status = if selected_backend.trim().is_empty() {
            format!("Starting runtime installation: {}", mode_name)
        } else {
            format!(
                "Starting runtime installation (backend: {}): {}",
                selected_backend.to_ascii_uppercase(),
                mode_name
            )
        };
        self.push_log(LogLevel::Info, self.runtime_status.clone());

        thread::spawn(move || {
            let result = install_runtime_asset(&asset, &runtime_dir, |status| {
                let _ = tx.send(BackgroundEvent::JobProgress { job_id, status });
            });
            let _ = tx.send(BackgroundEvent::RuntimeInstalled { job_id, result });
        });
    }

    fn runtime_maintenance_in_progress(&self) -> bool {
        self.runtime_download_in_progress || self.runtime_unblock_in_progress
    }

    fn start_runtime_unblock(&mut self) {
        if !runtime_unblock_required_for_platform() {
            self.runtime_status = "Unsigned runtime unblock is not required on Linux.".to_owned();
            self.status_message = self.runtime_status.clone();
            self.push_log(LogLevel::Info, self.runtime_status.clone());
            return;
        }

        if self.runtime_maintenance_in_progress() {
            return;
        }

        let runtime_dir = self.effective_runtime_dir();
        self.settings.runtime_dir = runtime_dir.display().to_string();
        self.runtime_unblock_in_progress = true;
        self.runtime_popup_open = true;
        self.runtime_status = "Running unsigned runtime unblock script...".to_owned();
        self.status_message = self.runtime_status.clone();
        self.push_log(LogLevel::Info, self.runtime_status.clone());

        let tx = self.background_tx.clone();
        let paths = self.paths.clone();
        let job_id = self.create_job(
            JobKind::RuntimeInstall,
            "Unblock unsigned runtime",
            JobState::Running,
            "Applying unsigned runtime unblock script...",
        );

        thread::spawn(move || {
            let result = run_unsigned_runtime_unblock_script(&paths, &runtime_dir);
            let _ = tx.send(BackgroundEvent::RuntimeUnblocked { job_id, result });
        });
    }

    fn start_device_enumeration(&mut self) {
        if self.device_enumeration_in_progress {
            return;
        }

        self.device_enumeration_in_progress = true;
        let runtime_dir = self.effective_runtime_dir();
        self.last_enumerated_runtime_dir = Some(runtime_dir.clone());
        let tx = self.background_tx.clone();
        let job_id = self.create_job(
            JobKind::DeviceEnumeration,
            "Enumerate runtime devices",
            JobState::Running,
            "Enumerating runtime devices...",
        );
        self.push_log(LogLevel::Info, "Started device enumeration.");

        thread::spawn(move || {
            let result = list_bridge_devices(&runtime_dir);
            let _ = tx.send(BackgroundEvent::DevicesEnumerated { job_id, result });
        });
    }

    fn start_model_download_with(
        &mut self,
        url: String,
        file_name: String,
        purpose: ModelDownloadPurpose,
    ) {
        if self.model_download_in_progress {
            return;
        }

        if url.is_empty() {
            self.runtime_status = "Model URL is empty.".to_owned();
            self.push_log(LogLevel::Warn, self.runtime_status.clone());
            return;
        }

        if file_name.trim().is_empty() {
            self.runtime_status = "Could not determine model file name.".to_owned();
            self.push_log(LogLevel::Warn, self.runtime_status.clone());
            return;
        }

        let destination = self.paths.models_dir.join(&file_name);
        self.model_download_in_progress = true;
        self.runtime_status = format!("Starting model download to '{}'.", destination.display());
        self.settings.model_download_file_name = file_name;
        let job_id = self.create_job(
            JobKind::ModelDownload,
            format!("Download model ({})", destination.display()),
            JobState::Running,
            format!("Downloading model to '{}'.", destination.display()),
        );
        self.push_log(LogLevel::Info, self.runtime_status.clone());

        let tx = self.background_tx.clone();
        thread::spawn(move || {
            let result = runtime_manager::download_model_to_file(&url, &destination, |status| {
                let _ = tx.send(BackgroundEvent::JobProgress { job_id, status });
            })
            .map(|_| destination);
            let _ = tx.send(BackgroundEvent::ModelDownloaded {
                job_id,
                purpose,
                result,
            });
        });
    }

    fn start_selected_model_combo_download(&mut self) {
        if self.model_download_in_progress {
            return;
        }

        let preset_index = self
            .selected_model_combo_preset
            .min(MODEL_COMBO_PRESETS.len().saturating_sub(1));
        let preset = Self::model_combo_preset(preset_index);
        let model_destination = self.paths.models_dir.join(preset.model_file);
        let mmproj_destination = self.paths.models_dir.join(preset.mmproj_file);
        self.runtime_status = format!(
            "Starting model combo download ({}) to '{}': '{}' then '{}'.",
            preset.label,
            self.paths.models_dir.display(),
            model_destination.display(),
            mmproj_destination.display()
        );
        self.status_message = self.runtime_status.clone();
        self.push_log(LogLevel::Info, self.runtime_status.clone());

        self.pending_mmproj_combo_preset = Some(preset_index);
        self.start_model_download_with(
            preset.model_url.to_owned(),
            preset.model_file.to_owned(),
            ModelDownloadPurpose::Model,
        );
    }

    fn save_settings_now(&mut self) -> Result<(), String> {
        save_settings(&self.paths, &self.settings)?;
        Ok(())
    }

    fn convert_selected_document(&mut self) {
        self.queue_checked_conversions();
    }

    fn pick_and_add_files(&mut self) {
        let files = rfd::FileDialog::new()
            .set_title("Add PDFs or images")
            .add_filter(
                "Documents",
                &[
                    "pdf", "png", "jpg", "jpeg", "bmp", "gif", "webp", "tif", "tiff",
                ],
            )
            .pick_files();
        if let Some(paths) = files {
            self.add_files(paths);
        }
    }

    fn adjust_pdf_zoom(&mut self, steps: i32) {
        let updated = self.settings.pdf_zoom + PDF_ZOOM_STEP * steps as f32;
        self.settings.pdf_zoom = updated.clamp(PDF_ZOOM_MIN, PDF_ZOOM_MAX);
    }

    fn conversion_output_path_for_source(
        input_path: &Path,
        mode_label: &str,
    ) -> Result<PathBuf, String> {
        let parent = input_path.parent().ok_or_else(|| {
            format!(
                "cannot determine output directory for '{}'",
                input_path.display()
            )
        })?;
        let stem = input_path
            .file_stem()
            .and_then(|value| value.to_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| format!("cannot determine file stem for '{}'", input_path.display()))?;
        Ok(parent.join(format!("{stem}{mode_label}.md")))
    }

    fn markdown_output_path_for_document(document: &WorkspaceDocument) -> Option<PathBuf> {
        match document.active_markdown_preview {
            MarkdownPreviewKind::Fast => document.fast_markdown_path.clone(),
            MarkdownPreviewKind::Vlm => document.vlm_markdown_path.clone(),
            MarkdownPreviewKind::Placeholder => None,
        }
    }

    fn document_markdown_variants(document: &WorkspaceDocument) -> String {
        let mut variants = Vec::new();
        if document.fast_markdown_path.is_some() {
            variants.push("FAST");
        }
        if document.vlm_markdown_path.is_some() {
            variants.push("VLM");
        }
        if variants.is_empty() {
            "MD: none".to_owned()
        } else {
            format!("MD: {}", variants.join(", "))
        }
    }

    fn document_markdown_is_dirty(document: &WorkspaceDocument) -> bool {
        document
            .markdown_edit_baseline
            .as_ref()
            .is_some_and(|baseline| baseline != &document.markdown)
    }

    fn begin_markdown_edit(document: &mut WorkspaceDocument) {
        if !document.markdown_edit_mode {
            document.markdown_edit_baseline = Some(document.markdown.clone());
            document.markdown_edit_mode = true;
        }
    }

    fn discard_markdown_edits(document: &mut WorkspaceDocument) {
        if let Some(baseline) = document.markdown_edit_baseline.take() {
            document.markdown = baseline;
        }
        document.markdown_edit_mode = false;
    }

    fn save_markdown_edits_for_index(&mut self, doc_index: usize) -> Result<(), String> {
        let Some(document) = self.documents.get_mut(doc_index) else {
            return Err("document index out of bounds".to_owned());
        };
        if !document.markdown_edit_mode {
            return Ok(());
        }
        if !Self::document_markdown_is_dirty(document) {
            return Ok(());
        }

        let Some(path) = Self::markdown_output_path_for_document(document) else {
            return Err(
                "Current preview has no markdown file. Convert first, then edit FAST/VLM markdown."
                    .to_owned(),
            );
        };

        let sanitized = sanitize_outer_markdown_fence(&document.markdown);
        std::fs::write(&path, sanitized.as_bytes()).map_err(|err| {
            format!(
                "failed writing markdown edits to '{}': {err}",
                path.to_string_lossy()
            )
        })?;
        document.markdown = sanitized;
        document.markdown_edit_baseline = Some(document.markdown.clone());
        self.markdown_cache.clear_scrollable();
        self.status_message = format!("Saved markdown edits to {}", path.display());
        self.push_log(LogLevel::Info, self.status_message.clone());
        Ok(())
    }

    fn selected_doc_index(&self) -> Option<usize> {
        self.selected_doc
            .filter(|index| *index < self.documents.len())
    }

    fn selected_doc_page_count(&self) -> usize {
        self.selected_doc_index()
            .map(|index| self.documents[index].page_count())
            .unwrap_or(1)
    }

    fn preferred_preview_kind_for_document(
        mode: &ConversionMode,
        is_pdf: bool,
    ) -> MarkdownPreviewKind {
        if !is_pdf {
            return MarkdownPreviewKind::Vlm;
        }
        match mode {
            ConversionMode::FastPdf => MarkdownPreviewKind::Fast,
            ConversionMode::PdfVlm => MarkdownPreviewKind::Vlm,
            ConversionMode::FastPdfWithVlmFallback => MarkdownPreviewKind::Fast,
        }
    }

    fn choose_preview_kind(
        current: MarkdownPreviewKind,
        preferred: MarkdownPreviewKind,
        has_fast: bool,
        has_vlm: bool,
        prefer_from_settings: bool,
    ) -> MarkdownPreviewKind {
        if prefer_from_settings {
            match preferred {
                MarkdownPreviewKind::Fast if has_fast => return MarkdownPreviewKind::Fast,
                MarkdownPreviewKind::Vlm if has_vlm => return MarkdownPreviewKind::Vlm,
                _ => {}
            }
            if has_fast {
                return MarkdownPreviewKind::Fast;
            }
            if has_vlm {
                return MarkdownPreviewKind::Vlm;
            }
            return MarkdownPreviewKind::Placeholder;
        }

        match current {
            MarkdownPreviewKind::Fast if has_fast => MarkdownPreviewKind::Fast,
            MarkdownPreviewKind::Vlm if has_vlm => MarkdownPreviewKind::Vlm,
            MarkdownPreviewKind::Placeholder if !has_fast && !has_vlm => {
                MarkdownPreviewKind::Placeholder
            }
            _ => {
                if has_fast {
                    MarkdownPreviewKind::Fast
                } else if has_vlm {
                    MarkdownPreviewKind::Vlm
                } else {
                    MarkdownPreviewKind::Placeholder
                }
            }
        }
    }

    fn set_document_markdown_preview(
        &mut self,
        doc_index: usize,
        preview: MarkdownPreviewKind,
    ) -> Result<(), String> {
        let Some(document) = self.documents.get(doc_index) else {
            return Err("document index out of bounds".to_owned());
        };
        let page_count = document.page_count();
        let doc_name = document.name.clone();
        let markdown = match preview {
            MarkdownPreviewKind::Placeholder => {
                build_unconverted_markdown_placeholder(page_count, &doc_name)
            }
            MarkdownPreviewKind::Fast => {
                let path = document
                    .fast_markdown_path
                    .as_ref()
                    .ok_or_else(|| "FAST markdown is not available for this file".to_owned())?;
                Self::normalized_markdown_output(path)?
            }
            MarkdownPreviewKind::Vlm => {
                let path = document
                    .vlm_markdown_path
                    .as_ref()
                    .ok_or_else(|| "VLM markdown is not available for this file".to_owned())?;
                Self::normalized_markdown_output(path)?
            }
        };

        if let Some(document) = self.documents.get_mut(doc_index) {
            document.markdown = markdown;
            document.markdown_edit_mode = false;
            document.markdown_edit_baseline = None;
            document.active_markdown_preview = preview;
        }
        self.markdown_cache.clear_scrollable();
        self.search_hits.clear();
        self.active_search_hit = 0;
        Ok(())
    }

    fn sync_document_markdown_sources(
        &mut self,
        doc_index: usize,
        prefer_from_settings: bool,
    ) -> Result<(), String> {
        let Some(document) = self.documents.get(doc_index) else {
            return Err("document index out of bounds".to_owned());
        };
        let doc_path = document.path.clone();
        let doc_name = document.name.clone();
        let doc_is_pdf = app_config::is_pdf(&doc_path);
        let page_count = document.page_count();
        let current_preview = document.active_markdown_preview;

        let fast_path = if doc_is_pdf {
            Self::conversion_output_path_for_source(&doc_path, "FAST")
                .ok()
                .filter(|path| path.exists())
        } else {
            None
        };
        let vlm_path = Self::conversion_output_path_for_source(&doc_path, "VLM")
            .ok()
            .filter(|path| path.exists());

        let preferred =
            Self::preferred_preview_kind_for_document(&self.settings.conversion_mode, doc_is_pdf);
        let selected_preview = Self::choose_preview_kind(
            current_preview,
            preferred,
            fast_path.is_some(),
            vlm_path.is_some(),
            prefer_from_settings,
        );

        if let Some(document) = self.documents.get_mut(doc_index) {
            document.fast_markdown_path = fast_path;
            document.vlm_markdown_path = vlm_path;
            document.active_markdown_preview = selected_preview;
        }

        if matches!(selected_preview, MarkdownPreviewKind::Placeholder) {
            if let Some(document) = self.documents.get_mut(doc_index) {
                document.markdown =
                    build_unconverted_markdown_placeholder(page_count, doc_name.as_str());
                document.markdown_edit_mode = false;
                document.markdown_edit_baseline = None;
            }
            self.markdown_cache.clear_scrollable();
            self.search_hits.clear();
            self.active_search_hit = 0;
            return Ok(());
        }

        self.set_document_markdown_preview(doc_index, selected_preview)
    }

    fn clamp_page_to_selected(&self, page_index: usize) -> usize {
        let count = self.selected_doc_page_count();
        page_index.min(count.saturating_sub(1))
    }

    fn set_current_page(&mut self, page_index: usize, sync_pdf: bool, sync_markdown: bool) {
        let clamped = self.clamp_page_to_selected(page_index);
        self.current_page = clamped;
        self.page_input = (clamped + 1).to_string();

        if sync_pdf {
            self.pending_sync_to_pdf = Some(clamped);
        }
        if sync_markdown {
            self.pending_sync_to_markdown = Some(clamped);
        }
    }

    fn select_document(&mut self, index: usize) {
        if index >= self.documents.len() {
            return;
        }

        self.selected_doc = Some(index);
        let _ = self.sync_document_markdown_sources(index, false);
        self.search_hits.clear();
        self.active_search_hit = 0;
        self.last_pdf_scroll_offset_y = 0.0;
        self.last_markdown_scroll_offset_y = 0.0;
        self.set_current_page(0, true, true);
        self.status_message = format!("Selected {}", self.documents[index].name);
    }

    fn remove_selected_document(&mut self) {
        let Some(index) = self.selected_doc_index() else {
            return;
        };

        let removed_name = self.documents[index].name.clone();
        self.documents.remove(index);

        self.search_hits.clear();
        self.active_search_hit = 0;

        if self.documents.is_empty() {
            self.selected_doc = None;
            self.current_page = 0;
            self.page_input = "1".to_owned();
            self.pending_sync_to_pdf = None;
            self.pending_sync_to_markdown = None;
            self.status_message = format!("Removed {}", removed_name);
            return;
        }

        let new_index = index.min(self.documents.len() - 1);
        self.selected_doc = Some(new_index);
        self.last_pdf_scroll_offset_y = 0.0;
        self.last_markdown_scroll_offset_y = 0.0;
        self.set_current_page(0, true, true);
        self.status_message = format!("Removed {}", removed_name);
    }

    fn add_files(&mut self, paths: Vec<PathBuf>) {
        if paths.is_empty() {
            return;
        }

        let runtime_dir = self.effective_runtime_dir();
        let mut scheduled = 0usize;
        let mut unsupported = Vec::new();

        for path in paths {
            let extension = path
                .extension()
                .and_then(|ext| ext.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            let supported = matches!(
                extension.as_str(),
                "pdf" | "png" | "jpg" | "jpeg" | "bmp" | "gif" | "webp" | "tif" | "tiff"
            );
            if !supported {
                unsupported.push(path);
                continue;
            }

            self.start_document_load_job(path, runtime_dir.clone());
            scheduled += 1;
        }

        if scheduled > 0 {
            self.status_message = format!("Loading {scheduled} file(s) in background...");
        }

        if !unsupported.is_empty() {
            let names = unsupported
                .iter()
                .map(|path| file_name_or_path(path))
                .collect::<Vec<_>>()
                .join(", ");
            let message = format!("Unsupported file type(s) skipped: {names}");
            self.push_log(LogLevel::Warn, message.clone());
            if scheduled == 0 {
                self.status_message = message;
            } else {
                self.status_message = format!("{} | {}", self.status_message, message);
            }
        } else if scheduled == 0 {
            self.status_message = "No files were added.".to_owned();
        }
    }

    fn start_document_load_job(&mut self, path: PathBuf, runtime_dir: PathBuf) {
        let name = file_name_or_path(&path);
        let job_id = self.create_job(
            JobKind::DocumentLoad,
            format!("Load {name}"),
            JobState::Running,
            "Preparing document load...",
        );
        self.push_log(
            LogLevel::Info,
            format!("Started loading '{}'.", path.display()),
        );
        let tx = self.background_tx.clone();

        thread::spawn(move || {
            let result = load_document_payload(&path, &runtime_dir, |status| {
                let _ = tx.send(BackgroundEvent::JobProgress { job_id, status });
            })
            .map(|kind| LoadedDocumentPayload {
                path: path.clone(),
                name: file_name_or_path(&path),
                kind,
            });
            let _ = tx.send(BackgroundEvent::DocumentLoaded { job_id, result });
        });
    }

    fn materialize_loaded_document(
        &mut self,
        ctx: &egui::Context,
        payload: LoadedDocumentPayload,
    ) -> Result<String, String> {
        let id = self.next_doc_id;
        self.next_doc_id += 1;

        let LoadedDocumentPayload { path, name, kind } = payload;
        let (kind, markdown) = match kind {
            LoadedDocumentKind::Pdf { pages, markdown } => {
                let mut ui_pages = Vec::with_capacity(pages.len());
                for page in pages {
                    let texture_name = format!("loaded_texture_{}", self.next_texture_id);
                    self.next_texture_id += 1;
                    let texture = load_rgba_texture(
                        ctx,
                        page.raster.width,
                        page.raster.height,
                        &page.raster.rgba,
                        texture_name,
                    );
                    let image_size = texture.size_vec2();
                    ui_pages.push(PdfPageData {
                        texture,
                        image_size,
                        text: page.text,
                    });
                }
                self.markdown_cache.clear_scrollable();
                (
                    DocumentKind::Pdf(PdfDocumentData { pages: ui_pages }),
                    markdown,
                )
            }
            LoadedDocumentKind::Image { raster, markdown } => {
                let texture_name = format!("loaded_texture_{}", self.next_texture_id);
                self.next_texture_id += 1;
                let texture =
                    load_rgba_texture(ctx, raster.width, raster.height, &raster.rgba, texture_name);
                let image_size = texture.size_vec2();
                (
                    DocumentKind::Image(ImageDocumentData {
                        texture,
                        image_size,
                    }),
                    markdown,
                )
            }
        };

        self.documents.push(WorkspaceDocument {
            id,
            path,
            name: name.clone(),
            markdown,
            markdown_edit_mode: false,
            markdown_edit_baseline: None,
            selected_for_conversion: true,
            fast_markdown_path: None,
            vlm_markdown_path: None,
            active_markdown_preview: MarkdownPreviewKind::Placeholder,
            kind,
        });

        let index = self.documents.len().saturating_sub(1);
        if let Err(err) = self.sync_document_markdown_sources(index, true) {
            self.push_log(
                LogLevel::Warn,
                format!(
                    "Could not load existing markdown preview for '{}': {}",
                    name, err
                ),
            );
        }
        if self.selected_doc.is_none() {
            self.select_document(index);
        }
        Ok(name)
    }

    fn run_search_for_selected(&mut self) {
        self.search_hits.clear();
        self.active_search_hit = 0;

        let Some(selected_index) = self.selected_doc_index() else {
            return;
        };

        let query = self.search_query.trim();
        if query.is_empty() {
            self.status_message = "Search cleared.".to_owned();
            return;
        }

        let query_lower = query.to_ascii_lowercase();
        let document = &self.documents[selected_index];
        let page_count = document.page_count();
        let markdown_by_page = split_markdown_by_page_markers(&document.markdown, page_count);

        match &document.kind {
            DocumentKind::Pdf(pdf_data) => {
                for (page_index, page) in pdf_data.pages.iter().enumerate() {
                    let pdf_hits = count_occurrences_case_insensitive(&page.text, &query_lower);
                    if pdf_hits == 0 {
                        continue;
                    }

                    let md_hits = markdown_by_page
                        .get(page_index)
                        .map(|content| count_occurrences_case_insensitive(content, &query_lower))
                        .unwrap_or(0);

                    self.search_hits.push(SearchHit {
                        page_index,
                        pdf_hits,
                        markdown_hits: md_hits,
                        excerpt: excerpt_from_text(&page.text, &query_lower),
                    });
                }
            }
            DocumentKind::Image(_) => {
                let md_hits = count_occurrences_case_insensitive(&document.markdown, &query_lower);
                if md_hits > 0 {
                    self.search_hits.push(SearchHit {
                        page_index: 0,
                        pdf_hits: 0,
                        markdown_hits: md_hits,
                        excerpt: excerpt_from_text(&document.markdown, &query_lower),
                    });
                }
            }
        }

        if self.search_hits.is_empty() {
            self.status_message = format!("No matches for \"{}\".", query);
            return;
        }

        let first_page = self.search_hits[0].page_index;
        self.set_current_page(first_page, true, true);
        self.status_message = format!("Found {} matching page(s).", self.search_hits.len());
    }

    fn jump_to_search_hit(&mut self, delta: isize) {
        if self.search_hits.is_empty() {
            return;
        }

        let len = self.search_hits.len() as isize;
        let current = self.active_search_hit as isize;
        let wrapped = (current + delta).rem_euclid(len);
        self.active_search_hit = wrapped as usize;
        let page = self.search_hits[self.active_search_hit].page_index;
        self.set_current_page(page, true, true);
    }

    fn ui_menu_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("menu_bar_panel")
            .resizable(false)
            .frame(
                egui::Frame::default()
                    .fill(Color32::from_rgb(242, 244, 247))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(210, 214, 221)))
                    .inner_margin(egui::Margin::symmetric(10, 6)),
            )
            .show(ctx, |ui| {
                egui::MenuBar::new().ui(ui, |ui| {
                    ui.menu_button("File", |ui| {
                        if native_menu_item(ui, "Add PDFs / Images").clicked() {
                            self.pick_and_add_files();
                            ui.close();
                        }
                        if native_menu_item(ui, "Remove Selected").clicked() {
                            self.remove_selected_document();
                            ui.close();
                        }
                        ui.separator();
                        if native_menu_item(ui, "Quit").clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            ui.close();
                        }
                    });

                    ui.menu_button("View", |ui| {
                        if native_menu_item(ui, "Zoom larger").clicked() {
                            self.adjust_pdf_zoom(1);
                            ctx.request_repaint();
                            ui.close();
                        }
                        if native_menu_item(ui, "Zoom smaller").clicked() {
                            self.adjust_pdf_zoom(-1);
                            ctx.request_repaint();
                            ui.close();
                        }
                        if native_menu_item(ui, "Reset zoom (100%)").clicked() {
                            self.settings.pdf_zoom = 1.0;
                            ctx.request_repaint();
                            ui.close();
                        }
                    });

                    ui.menu_button("Settings", |ui| {
                        if native_menu_item(ui, "Runtime / VLM settings").clicked() {
                            self.settings_window_open = true;
                            ui.close();
                        }
                        if native_menu_item(ui, "Prompt overrides").clicked() {
                            self.prompt_overrides_window_open = true;
                            ui.close();
                        }
                        if native_menu_item(ui, "Jobs and logs").clicked() {
                            self.logs_window_open = true;
                            ui.close();
                        }
                        ui.separator();
                        if native_menu_item(ui, "Save settings now").clicked() {
                            match self.save_settings_now() {
                                Ok(()) => {
                                    self.status_message = "Settings saved.".to_owned();
                                    self.push_log(LogLevel::Info, self.status_message.clone());
                                }
                                Err(err) => {
                                    self.status_message = format!("Failed to save settings: {err}");
                                    self.push_log(LogLevel::Error, self.status_message.clone());
                                }
                            }
                            ui.close();
                        }
                    });

                    ui.menu_button("Help", |ui| {
                        if native_menu_item(ui, "About").clicked() {
                            self.about_window_open = true;
                            ui.close();
                        }
                        if native_menu_item(ui, "Notices").clicked() {
                            self.open_legal_doc(LegalDocKind::ThirdPartyNotices);
                            ui.close();
                        }
                        if native_menu_item(ui, "Third-party licenses").clicked() {
                            self.open_legal_doc(LegalDocKind::ThirdPartyLicenses);
                            ui.close();
                        }
                        if native_menu_item(ui, "Engine licenses").clicked() {
                            self.open_legal_doc(LegalDocKind::EngineThirdPartyLicenses);
                            ui.close();
                        }
                    });
                });
            });
    }

    fn ui_top_panel(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top_panel")
            .resizable(false)
            .frame(
                egui::Frame::default()
                    .fill(Color32::from_rgb(246, 247, 249))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(219, 221, 226)))
                    .inner_margin(egui::Margin::symmetric(14, 12)),
            )
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.heading("PDF Markdown Studio");
                    ui.separator();
                    ui.label("Mode");
                    egui::ComboBox::from_id_salt("conversion_mode")
                        .selected_text(match self.settings.conversion_mode {
                            ConversionMode::FastPdf => "Fast PDF (pdf.dll)",
                            ConversionMode::PdfVlm => "PDF VLM + Image VLM",
                            ConversionMode::FastPdfWithVlmFallback => "FAST with VLM fallback",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.settings.conversion_mode,
                                ConversionMode::FastPdf,
                                "Fast PDF (pdf.dll)",
                            )
                            .on_hover_text(
                                "Machine-readable PDF text extraction via pdf.dll. Fastest path and no VLM model required.",
                            );
                            ui.selectable_value(
                                &mut self.settings.conversion_mode,
                                ConversionMode::PdfVlm,
                                "PDF VLM + Image VLM",
                            )
                            .on_hover_text(
                                "PDFs use PDFVLM. Images are always routed to VLM image mode with your image prompt.",
                            );
                            ui.selectable_value(
                                &mut self.settings.conversion_mode,
                                ConversionMode::FastPdfWithVlmFallback,
                                "FAST with VLM fallback",
                            )
                            .on_hover_text(
                                "Try FAST first; if PDF is not machine-readable, automatically rerun that PDF with VLM.",
                            );
                        });
                    ui.separator();
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("Convert Selected").color(Color32::WHITE),
                            )
                            .fill(Color32::from_rgb(35, 121, 90))
                            .stroke(Stroke::new(1.0, Color32::from_rgb(28, 94, 70))),
                        )
                        .clicked()
                    {
                        self.convert_selected_document();
                    }
                });
            });
    }

    fn ui_documents_sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("documents_sidebar")
            .resizable(true)
            .default_width(300.0)
            .min_width(220.0)
            .max_width(460.0)
            .frame(
                egui::Frame::default()
                    .fill(Color32::from_rgb(247, 248, 250))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(219, 221, 227)))
                    .inner_margin(egui::Margin::same(10)),
            )
            .show(ctx, |ui| {
                let checked_count = self
                    .documents
                    .iter()
                    .filter(|doc| doc.selected_for_conversion)
                    .count();
                ui.horizontal(|ui| {
                    ui.heading("Documents");
                    ui.separator();
                    ui.label(
                        RichText::new(format!("{}", self.documents.len()))
                            .strong()
                            .color(Color32::from_rgb(45, 74, 61)),
                    );
                    ui.separator();
                    ui.label(
                        RichText::new(format!("Checked: {checked_count}"))
                            .small()
                            .color(Color32::from_rgb(77, 83, 94)),
                    );
                });
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Select All").clicked() {
                        for document in &mut self.documents {
                            document.selected_for_conversion = true;
                        }
                    }
                    if ui.button("Select None").clicked() {
                        for document in &mut self.documents {
                            document.selected_for_conversion = false;
                        }
                    }
                });
                ui.add_space(4.0);

                if self.documents.is_empty() {
                    ui.label("No files loaded.");
                    return;
                }

                ScrollArea::vertical()
                    .id_salt("documents_sidebar_list")
                    .show(ui, |ui| {
                        let mut new_selection = None;
                        for index in 0..self.documents.len() {
                            let selected = self.selected_doc_index() == Some(index);
                            let document = &mut self.documents[index];
                            let title = format!(
                                "{}  [{}]",
                                document.name,
                                document.doc_type_label().to_uppercase()
                            );
                            let meta = format!(
                                "{} page{}",
                                document.page_count(),
                                if document.page_count() == 1 { "" } else { "s" }
                            );
                            let markdown_status = Self::document_markdown_variants(document);
                            let frame = if selected {
                                egui::Frame::group(ui.style())
                                    .fill(Color32::from_rgb(227, 236, 232))
                                    .stroke(Stroke::new(1.0, Color32::from_rgb(139, 166, 152)))
                                    .corner_radius(CornerRadius::same(8))
                                    .inner_margin(egui::Margin::same(8))
                            } else {
                                egui::Frame::group(ui.style())
                                    .fill(Color32::from_rgb(252, 252, 253))
                                    .stroke(Stroke::new(1.0, Color32::from_rgb(221, 223, 228)))
                                    .corner_radius(CornerRadius::same(8))
                                    .inner_margin(egui::Margin::same(8))
                            };
                            let mut request_selection = false;
                            let response = frame
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.checkbox(&mut document.selected_for_conversion, "");
                                        let title_response = ui.add(
                                            egui::Label::new(
                                                RichText::new(title)
                                                    .strong()
                                                    .color(Color32::from_rgb(40, 44, 51)),
                                            )
                                            .sense(egui::Sense::click())
                                            .wrap(),
                                        );
                                        if title_response.clicked() {
                                            request_selection = true;
                                        }
                                    });
                                    let meta_response = ui.add(
                                        egui::Label::new(
                                            RichText::new(meta)
                                                .small()
                                                .color(Color32::from_rgb(102, 107, 116)),
                                        )
                                        .sense(egui::Sense::click()),
                                    );
                                    if meta_response.clicked() {
                                        request_selection = true;
                                    }
                                    let status_response = ui.add(
                                        egui::Label::new(
                                            RichText::new(markdown_status)
                                                .small()
                                                .color(Color32::from_rgb(63, 105, 90)),
                                        )
                                        .sense(egui::Sense::click()),
                                    );
                                    if status_response.clicked() {
                                        request_selection = true;
                                    }
                                })
                                .response;
                            if request_selection {
                                new_selection = Some(index);
                            }
                            response.on_hover_text(document.path.to_string_lossy());
                            ui.add_space(4.0);
                        }
                        if let Some(index) = new_selection {
                            self.select_document(index);
                        }
                    });

                ui.separator();
                if let Some(index) = self.selected_doc_index() {
                    let doc = &self.documents[index];
                    ui.label(RichText::new("Selected").strong());
                    ui.label(
                        RichText::new(format!("Type: {}", doc.doc_type_label()))
                            .small()
                            .color(Color32::from_rgb(97, 102, 113)),
                    );
                    ui.label(
                        RichText::new(format!("Path: {}", doc.path.display()))
                            .small()
                            .color(Color32::from_rgb(97, 102, 113)),
                    );
                    ui.label(
                        RichText::new(Self::document_markdown_variants(doc))
                            .small()
                            .color(Color32::from_rgb(63, 105, 90)),
                    );
                }
            });
    }

    fn ui_search_bar(&mut self, ui: &mut egui::Ui) {
        let has_selected = self.selected_doc_index().is_some();
        if !has_selected {
            ui.label("Add a PDF or image to start.");
            return;
        }

        let mut run_search = false;
        let mut run_go_to_page = false;
        let mut markdown_preview_to_apply: Option<MarkdownPreviewKind> = None;

        ui.horizontal_wrapped(|ui| {
            ui.label("Search");
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.search_query)
                    .hint_text("Search in PDF text, then in matching markdown page block"),
            );
            if response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                run_search = true;
            }
            if ui.button("Find").clicked() {
                run_search = true;
            }
            if ui.button("Prev Hit").clicked() {
                self.jump_to_search_hit(-1);
            }
            if ui.button("Next Hit").clicked() {
                self.jump_to_search_hit(1);
            }

            ui.separator();
            if ui.button("Prev Page").clicked() {
                let page = self.current_page.saturating_sub(1);
                self.set_current_page(page, true, true);
            }
            if ui.button("Next Page").clicked() {
                let page = self.current_page + 1;
                self.set_current_page(page, true, true);
            }
            ui.label("Go to page");
            let go_response = ui.add(
                egui::TextEdit::singleline(&mut self.page_input)
                    .desired_width(65.0)
                    .hint_text("1"),
            );
            if go_response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                run_go_to_page = true;
            }
            if ui.button("Go").clicked() {
                run_go_to_page = true;
            }
            ui.separator();
            ui.label(format!("Current page: {}", self.current_page + 1));

            if let Some(selected_index) = self.selected_doc_index() {
                let document = &self.documents[selected_index];
                if document.fast_markdown_path.is_some() && document.vlm_markdown_path.is_some() {
                    ui.separator();
                    ui.label("Markdown");
                    let mut selected_preview = document.active_markdown_preview;
                    egui::ComboBox::from_id_salt("markdown_preview_selector")
                        .selected_text(selected_preview.label())
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut selected_preview,
                                MarkdownPreviewKind::Fast,
                                MarkdownPreviewKind::Fast.label(),
                            );
                            ui.selectable_value(
                                &mut selected_preview,
                                MarkdownPreviewKind::Vlm,
                                MarkdownPreviewKind::Vlm.label(),
                            );
                        });
                    if selected_preview != document.active_markdown_preview {
                        markdown_preview_to_apply = Some(selected_preview);
                    }
                }
            }
        });

        if let (Some(selected_index), Some(preview)) =
            (self.selected_doc_index(), markdown_preview_to_apply)
        {
            match self.set_document_markdown_preview(selected_index, preview) {
                Ok(()) => {
                    self.status_message =
                        format!("Switched markdown preview to {}.", preview.label());
                }
                Err(err) => {
                    self.status_message = format!("Failed to switch markdown preview: {err}");
                    self.push_log(LogLevel::Error, self.status_message.clone());
                }
            }
        }

        if run_search {
            self.run_search_for_selected();
        }

        if run_go_to_page {
            if let Ok(page_number) = self.page_input.trim().parse::<usize>() {
                let page_index = page_number.saturating_sub(1);
                self.set_current_page(page_index, true, true);
            }
        }

        if !self.search_hits.is_empty() {
            let mut clicked_index = None;
            ui.horizontal_wrapped(|ui| {
                ui.label(format!(
                    "{} hit page(s). Active: {}/{}",
                    self.search_hits.len(),
                    self.active_search_hit + 1,
                    self.search_hits.len()
                ));

                for (index, hit) in self.search_hits.iter().enumerate() {
                    let selected = self.active_search_hit == index;
                    let label = format!(
                        "p{} (pdf {}, md {})",
                        hit.page_index + 1,
                        hit.pdf_hits,
                        hit.markdown_hits
                    );

                    let response = ui.selectable_label(selected, label);
                    if response.clicked() {
                        clicked_index = Some(index);
                    }
                    response.on_hover_text(&hit.excerpt);
                }
            });

            if let Some(index) = clicked_index {
                self.active_search_hit = index;
                let page = self.search_hits[index].page_index;
                self.set_current_page(page, true, true);
            }
        }
    }

    fn ui_split_view(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        selected_index: usize,
        viewport_width: f32,
        viewport_height: f32,
    ) {
        let mut pending_pdf_sync = self.pending_sync_to_pdf;
        let mut pending_md_sync = self.pending_sync_to_markdown;
        let current_page = self.current_page;
        let pdf_zoom = self.settings.pdf_zoom;
        let markdown_font_size = (self.settings.markdown_font_size * pdf_zoom).clamp(9.0, 72.0);

        let mut left_metrics = PaneMetrics::default();
        let mut right_metrics = PaneMetrics::default();
        let mut markdown_ui_actions = MarkdownPaneUiActions::default();
        let enforce_page_height_sync = false;

        let viewport_width = viewport_width.max(1.0);
        let viewport_height = viewport_height.max(1.0);
        // Grow the entire side-by-side pane pair with zoom so outer horizontal scrolling can handle overflow.
        let base_total_width = (viewport_width - 12.0).max(1.0);
        let total_width = (base_total_width * pdf_zoom).clamp(160.0, 12_000.0);
        let pane_gap = 8.0;
        let split_width = (total_width - pane_gap).max(0.0);
        let left_pane_width = (split_width * 0.5).floor();
        let right_pane_width = (split_width - left_pane_width).max(0.0);
        let total_height = viewport_height.clamp(260.0, 12_000.0);
        let split_size = Vec2::new(total_width, total_height);
        if (split_size.x - self.last_split_view_size.x).abs() > 0.5
            || (split_size.y - self.last_split_view_size.y).abs() > 0.5
        {
            pending_pdf_sync.get_or_insert(current_page);
            pending_md_sync.get_or_insert(current_page);
            self.last_split_view_size = split_size;
        }

        ui.allocate_ui_with_layout(
            Vec2::new(total_width, total_height),
            egui::Layout::left_to_right(egui::Align::Min),
            |ui| {
                ui.allocate_ui_with_layout(
                    Vec2::new(left_pane_width, total_height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(left_pane_width.max(1.0));
                        ui.set_min_width(left_pane_width.max(1.0));
                        ui.set_max_width(left_pane_width.max(1.0));
                        let document = &self.documents[selected_index];
                        egui::Frame::group(ui.style())
                            .fill(Color32::from_rgb(252, 252, 253))
                            .stroke(Stroke::new(1.0, Color32::from_rgb(220, 222, 228)))
                            .corner_radius(CornerRadius::same(10))
                            .inner_margin(egui::Margin::same(10))
                            .show(ui, |ui| {
                                left_metrics = render_source_pane(
                                    ui,
                                    ctx,
                                    document,
                                    current_page,
                                    &mut pending_pdf_sync,
                                );
                            });
                    },
                );

                ui.add_space(pane_gap);

                ui.allocate_ui_with_layout(
                    Vec2::new(right_pane_width, total_height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(right_pane_width.max(1.0));
                        ui.set_min_width(right_pane_width.max(1.0));
                        ui.set_max_width(right_pane_width.max(1.0));
                        let document = &mut self.documents[selected_index];
                        egui::Frame::group(ui.style())
                            .fill(Color32::from_rgb(252, 252, 253))
                            .stroke(Stroke::new(1.0, Color32::from_rgb(220, 222, 228)))
                            .corner_radius(CornerRadius::same(10))
                            .inner_margin(egui::Margin::same(10))
                            .show(ui, |ui| {
                                right_metrics = render_markdown_pane(
                                    ui,
                                    ctx,
                                    document,
                                    current_page,
                                    markdown_font_size,
                                    &[],
                                    enforce_page_height_sync,
                                    &mut self.pending_markdown_edit_focus_page,
                                    &mut self.last_markdown_toggle_at,
                                    &mut self.markdown_cache,
                                    &mut pending_md_sync,
                                    &mut markdown_ui_actions,
                                );
                            });
                    },
                );
            },
        );

        if markdown_ui_actions.request_enter_edit_mode {
            let edit_page = markdown_ui_actions
                .request_enter_edit_mode_page
                .unwrap_or(current_page);
            self.set_current_page(edit_page, true, false);
            if let Some(document) = self.documents.get_mut(selected_index) {
                Self::begin_markdown_edit(document);
            }
            self.pending_markdown_edit_focus_page = Some(edit_page);
        }
        if markdown_ui_actions.request_save_edits {
            if let Err(err) = self.save_markdown_edits_for_index(selected_index) {
                self.status_message = err.clone();
                self.push_log(LogLevel::Error, err);
            }
        }
        if markdown_ui_actions.request_exit_edit_mode
            && let Some(document) = self.documents.get(selected_index)
        {
            if Self::document_markdown_is_dirty(document) {
                self.pending_markdown_exit_doc_id = Some(document.id);
            } else if let Some(document) = self.documents.get_mut(selected_index) {
                document.markdown_edit_mode = false;
                document.markdown_edit_baseline = None;
            }
        }

        self.pending_sync_to_pdf = pending_pdf_sync;
        self.pending_sync_to_markdown = pending_md_sync;

        let left_scroll_delta =
            (left_metrics.scroll_offset_y - self.last_pdf_scroll_offset_y).abs();
        let right_scroll_delta =
            (right_metrics.scroll_offset_y - self.last_markdown_scroll_offset_y).abs();
        let left_scroll_changed = left_scroll_delta > 0.5;
        let right_scroll_changed = right_scroll_delta > 0.5;
        self.last_pdf_scroll_offset_y = left_metrics.scroll_offset_y;
        self.last_markdown_scroll_offset_y = right_metrics.scroll_offset_y;
        let mut left_driving = left_metrics.hovered
            && (left_metrics.user_scrolled
                || (left_scroll_changed && self.pending_sync_to_pdf.is_none()));
        let mut right_driving = right_metrics.hovered
            && (right_metrics.user_scrolled
                || (right_scroll_changed && self.pending_sync_to_markdown.is_none()));
        if left_driving && right_driving {
            if left_scroll_delta >= right_scroll_delta {
                right_driving = false;
            } else {
                left_driving = false;
            }
        }

        if left_driving && !right_driving {
            if let Some(page) = left_metrics.first_visible_page {
                if page != self.current_page {
                    self.set_current_page(page, false, false);
                    self.pending_sync_to_markdown = Some(page);
                }
            }
        } else if right_driving && !left_driving {
            if let Some(page) = right_metrics.first_visible_page {
                if page != self.current_page {
                    self.set_current_page(page, false, false);
                    self.pending_sync_to_pdf = Some(page);
                }
            }
        }

        if self.pending_sync_to_pdf.is_some() || self.pending_sync_to_markdown.is_some() {
            ctx.request_repaint();
        }
    }

    fn ui_markdown_unsaved_dialog(&mut self, ctx: &egui::Context) {
        let Some(doc_id) = self.pending_markdown_exit_doc_id else {
            return;
        };
        let Some(doc_index) = self
            .documents
            .iter()
            .position(|document| document.id == doc_id)
        else {
            self.pending_markdown_exit_doc_id = None;
            return;
        };

        let doc_name = self.documents[doc_index].name.clone();
        let mut open = true;
        let mut requested_save = false;
        let mut requested_discard = false;
        let mut requested_cancel = false;

        egui::Window::new("Unsaved Markdown Edits")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label(format!("'{}' has unsaved markdown edits.", doc_name));
                ui.label("Save before leaving edit mode?");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::Button::new(RichText::new("Save").color(Color32::WHITE))
                                .fill(Color32::from_rgb(35, 121, 90))
                                .stroke(Stroke::new(1.0, Color32::from_rgb(28, 94, 70))),
                        )
                        .clicked()
                    {
                        requested_save = true;
                    }
                    if ui.button("Discard").clicked() {
                        requested_discard = true;
                    }
                    if ui.button("Cancel").clicked() {
                        requested_cancel = true;
                    }
                });
            });

        if requested_save {
            match self.save_markdown_edits_for_index(doc_index) {
                Ok(()) => {
                    if let Some(document) = self.documents.get_mut(doc_index) {
                        document.markdown_edit_mode = false;
                        document.markdown_edit_baseline = None;
                    }
                    self.pending_markdown_exit_doc_id = None;
                }
                Err(err) => {
                    self.status_message = err.clone();
                    self.push_log(LogLevel::Error, err);
                    self.pending_markdown_exit_doc_id = None;
                }
            }
            return;
        }

        if requested_discard {
            if let Some(document) = self.documents.get_mut(doc_index) {
                Self::discard_markdown_edits(document);
            }
            self.markdown_cache.clear_scrollable();
            self.pending_markdown_exit_doc_id = None;
            return;
        }

        if requested_cancel || !open {
            self.pending_markdown_exit_doc_id = None;
        }
    }

    fn ui_prompt_overrides_window(&mut self, ctx: &egui::Context) {
        if !self.prompt_overrides_window_open {
            return;
        }

        let was_open = self.prompt_overrides_window_open;
        let mut save_attempted = false;
        let mut save_failed = false;
        let mut request_close = false;
        let mut open = self.prompt_overrides_window_open;
        egui::Window::new("Prompt Overrides")
            .open(&mut open)
            .vscroll(true)
            .default_size([860.0, 640.0])
            .show(ctx, |ui| {
                ui.label(
                    RichText::new("Override prompts for VLM conversions. These prompts are sent to the engine binary as-is.")
                        .small()
                        .color(Color32::from_rgb(88, 94, 104)),
                );
                ui.label(
                    RichText::new(format!(
                        "Saved in: {}",
                        self.paths.settings_json.display()
                    ))
                    .small()
                    .color(Color32::from_rgb(88, 94, 104)),
                );
                ui.separator();

                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("PDF prompt").strong());
                    if ui.button("Reset PDF prompt to default").clicked() {
                        self.settings.vlm_prompt = DEFAULT_VLM_PROMPT.to_owned();
                    }
                });
                ui.add(
                    egui::TextEdit::multiline(&mut self.settings.vlm_prompt)
                        .desired_rows(9)
                        .desired_width(f32::INFINITY),
                );

                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("Image prompt").strong());
                    if ui.button("Reset image prompt to default").clicked() {
                        self.settings.vlm_image_prompt = DEFAULT_IMAGE_VLM_PROMPT.to_owned();
                    }
                });
                ui.add(
                    egui::TextEdit::multiline(&mut self.settings.vlm_image_prompt)
                        .desired_rows(9)
                        .desired_width(f32::INFINITY),
                );

                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Reset both prompts to defaults").clicked() {
                        self.settings.vlm_prompt = DEFAULT_VLM_PROMPT.to_owned();
                        self.settings.vlm_image_prompt = DEFAULT_IMAGE_VLM_PROMPT.to_owned();
                    }
                    if ui.button("Save prompt overrides").clicked() {
                        save_attempted = true;
                        match self.save_settings_now() {
                            Ok(()) => {
                                self.status_message = "Prompt overrides saved.".to_owned();
                                self.push_log(LogLevel::Info, "Prompt overrides saved.");
                            }
                            Err(err) => {
                                save_failed = true;
                                self.status_message = format!("Failed saving prompts: {err}");
                                self.push_log(LogLevel::Error, self.status_message.clone());
                            }
                        }
                    }
                    if ui.button("Save + Close").clicked() {
                        save_attempted = true;
                        match self.save_settings_now() {
                            Ok(()) => {
                                self.status_message = "Prompt overrides saved.".to_owned();
                                self.push_log(LogLevel::Info, "Prompt overrides saved.");
                                request_close = true;
                            }
                            Err(err) => {
                                save_failed = true;
                                self.status_message = format!("Failed saving prompts: {err}");
                                self.push_log(LogLevel::Error, self.status_message.clone());
                            }
                        }
                    }
                    if ui.button("Close").clicked() {
                        request_close = true;
                    }
                });
            });

        if request_close {
            open = false;
        }
        self.prompt_overrides_window_open = open;
        if was_open && !self.prompt_overrides_window_open && !save_attempted {
            match self.save_settings_now() {
                Ok(()) => {
                    self.status_message = "Prompt overrides saved.".to_owned();
                    self.push_log(LogLevel::Info, "Prompt overrides saved.");
                }
                Err(err) => {
                    save_failed = true;
                    self.status_message = format!("Failed saving prompts: {err}");
                    self.push_log(LogLevel::Error, self.status_message.clone());
                }
            }
        }
        if save_attempted && save_failed {
            self.prompt_overrides_window_open = true;
        }
    }

    fn ui_about_window(&mut self, ctx: &egui::Context) {
        if !self.about_window_open {
            return;
        }

        let mut open = self.about_window_open;
        egui::Window::new("About PDF Markdown Studio")
            .collapsible(false)
            .resizable(true)
            .default_size([880.0, 620.0])
            .min_size([680.0, 420.0])
            .open(&mut open)
            .show(ctx, |ui| {
                ui.heading("PDF Markdown Studio");
                ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                ui.label("Native desktop GUI around OpenResearchTools engine runtime for PDF/Image to Markdown conversion.");
                ui.label("Copyright (c) 2026 L. Rutkauskas");
                ui.separator();
                ui.label("What this app does:");
                ui.label("- Local PDF and image loading with side-by-side source/markdown review.");
                ui.label("- Machine-readable PDF extraction via FAST mode.");
                ui.label("- VLM conversion via runtime PDFVLM and image VLM paths.");
                ui.label("- Per-document conversion queueing, prompt overrides, runtime setup, and GPU routing controls.");
                ui.separator();
                ui.label("Bundled legal documents (embedded in this executable):");
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Notices").clicked() {
                        self.open_legal_doc(LegalDocKind::ThirdPartyNotices);
                    }
                    if ui.button("Third-party licenses").clicked() {
                        self.open_legal_doc(LegalDocKind::ThirdPartyLicenses);
                    }
                    if ui.button("Engine licenses").clicked() {
                        self.open_legal_doc(LegalDocKind::EngineThirdPartyLicenses);
                    }
                });
                ui.separator();
                ui.label("PDF Markdown Studio license:");
                let row_height = ui.text_style_height(&egui::TextStyle::Monospace);
                let license_lines: Vec<&str> = BUNDLED_PDF_MARKDOWN_STUDIO_LICENSE_TXT.lines().collect();
                ScrollArea::vertical()
                    .id_salt("about_app_license_scroll")
                    .max_height(260.0)
                    .auto_shrink([false, false])
                    .show_rows(ui, row_height, license_lines.len(), |ui, row_range| {
                        for row in row_range {
                            if let Some(line) = license_lines.get(row) {
                                ui.add(
                                    egui::Label::new(egui::RichText::new(*line).monospace())
                                        .wrap_mode(egui::TextWrapMode::Extend),
                                );
                            }
                        }
                    });
                ui.separator();
                ui.label(
                    RichText::new("Model/runtime legal obligations are governed by upstream licenses and notices included in these bundled documents.")
                        .small()
                        .color(Color32::from_rgb(92, 96, 107)),
                );
            });
        self.about_window_open = open;
    }

    fn ui_legal_docs_window(&mut self, ctx: &egui::Context) {
        if !self.legal_docs_window_open {
            return;
        }

        let mut still_open = true;
        let viewport_id = egui::ViewportId::from_hash_of("legal-docs-window");
        let window_title = format!(
            "PDF Markdown Studio - {}",
            self.legal_doc_kind.window_title()
        );
        let builder = with_app_icon(
            egui::ViewportBuilder::default()
                .with_title(window_title)
                .with_inner_size([1120.0, 760.0])
                .with_resizable(true),
        );

        ctx.show_viewport_immediate(viewport_id, builder, |ctx, _class| {
            if ctx.input(|i| i.viewport().close_requested()) {
                still_open = false;
            }

            egui::CentralPanel::default().show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Close").clicked() {
                            still_open = false;
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                    for kind in LegalDocKind::ALL {
                        if ui
                            .selectable_label(self.legal_doc_kind == kind, kind.nav_label())
                            .clicked()
                        {
                            self.set_legal_doc_kind(kind);
                        }
                    }
                });
                ui.separator();
                ui.label(format!("Document: {}", self.legal_doc_kind.window_title()));
                ui.separator();
                let row_height = ui.text_style_height(&egui::TextStyle::Monospace);
                ScrollArea::both()
                    .id_salt("legal_docs_scroll")
                    .auto_shrink([false, false])
                    .show_rows(
                        ui,
                        row_height,
                        self.legal_doc_lines.len(),
                        |ui, row_range| {
                            for row in row_range {
                                if let Some(line) = self.legal_doc_lines.get(row) {
                                    ui.add(
                                        egui::Label::new(egui::RichText::new(line).monospace())
                                            .wrap_mode(egui::TextWrapMode::Extend),
                                    );
                                }
                            }
                        },
                    );
            });
        });

        self.legal_docs_window_open = still_open;
    }

    fn ui_settings_window(&mut self, ctx: &egui::Context) {
        if !self.settings_window_open && !self.runtime_popup_open {
            return;
        }

        self.refresh_runtime_state();
        self.ensure_devices_enumerated_for_runtime();
        let setup_required = self.runtime_popup_open;
        let runtime_ok = self.runtime_check.is_ok();
        let vlm_model_ready = self.vlm_model_ready();
        let vlm_mmproj_ready = self.vlm_mmproj_ready();

        let mut open = self.settings_window_open || self.runtime_popup_open;
        let mut dismiss_setup_popup = false;
        egui::Window::new(if setup_required {
            "Setup Required"
        } else {
            "Settings"
        })
            .open(&mut open)
            .collapsible(!setup_required)
            .vscroll(true)
            .default_size([760.0, 620.0])
            .show(ctx, |ui| {
                if setup_required {
                    egui::Frame::group(ui.style())
                        .fill(Color32::from_rgb(255, 242, 242))
                        .stroke(Stroke::new(1.0, Color32::from_rgb(221, 111, 111)))
                        .corner_radius(CornerRadius::same(8))
                        .inner_margin(egui::Margin::same(10))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new("Essential setup is incomplete.")
                                    .strong()
                                    .color(Color32::from_rgb(167, 37, 37)),
                            );
                            if !runtime_ok {
                                ui.label(
                                    RichText::new("Runtime: missing required engine files")
                                        .color(Color32::from_rgb(167, 37, 37)),
                                );
                                for missing in &self.runtime_check.missing {
                                    ui.label(
                                        RichText::new(format!("- {}", missing))
                                            .color(Color32::from_rgb(167, 37, 37)),
                                    );
                                }
                            } else {
                                ui.label(
                                    RichText::new("Runtime: OK")
                                        .color(Color32::from_rgb(36, 128, 79)),
                                );
                            }

                            if !vlm_model_ready {
                                ui.label(
                                    RichText::new("VLM model GGUF: missing or invalid path")
                                        .color(Color32::from_rgb(167, 37, 37)),
                                );
                            } else {
                                ui.label(
                                    RichText::new("VLM model GGUF: OK")
                                        .color(Color32::from_rgb(36, 128, 79)),
                                );
                            }

                            if !vlm_mmproj_ready {
                                ui.label(
                                    RichText::new("MMProj GGUF: missing or invalid path")
                                        .color(Color32::from_rgb(167, 37, 37)),
                                );
                            } else {
                                ui.label(
                                    RichText::new("MMProj GGUF: OK")
                                        .color(Color32::from_rgb(36, 128, 79)),
                                );
                            }

                            ui.separator();
                            ui.label(
                                RichText::new("FAST PDF for machine-readable digital PDFs can run without VLM models. PDF VLM, fallback mode, and images require both model GGUF + MMProj GGUF.")
                                    .small()
                                    .color(Color32::from_rgb(99, 66, 66)),
                            );
                            if self.runtime_post_install_prompt {
                                ui.colored_label(
                                    Color32::from_rgb(154, 103, 0),
                                    "Runtime was just installed. Click 'Unblock unsigned runtime', then reload runtime check.",
                                );
                            }
                            ui.horizontal_wrapped(|ui| {
                                if ui.button("Continue (FAST-only for now)").clicked() {
                                    dismiss_setup_popup = true;
                                }
                            });
                        });
                    ui.add_space(8.0);
                }

                ui.heading("Runtime");
                ui.label("Canonical app storage (single root):");
                ui.monospace(self.paths.app_data_dir.display().to_string());
                ui.label("Default runtime path:");
                ui.monospace(self.paths.runtime_shared_dir.display().to_string());
                ui.label("Settings file:");
                ui.monospace(self.paths.settings_json.display().to_string());
                ui.label("Models directory:");
                ui.monospace(self.paths.models_dir.display().to_string());
                ui.label("Conversion output:");
                ui.label("Saved next to each source file as <source>FAST.md or <source>VLM.md");
                ui.label(
                    RichText::new("Note: to switch to a different runtime variant, first close all apps that use the shared runtime, delete the engine runtime folder, reopen this app, then download/repair the desired runtime.")
                        .small()
                        .color(Color32::from_rgb(89, 95, 105)),
                );
                ui.horizontal(|ui| {
                    ui.label("Runtime dir");
                    ui.text_edit_singleline(&mut self.settings.runtime_dir);
                    if ui.button("Browse").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .set_title("Select runtime folder")
                            .pick_folder()
                        {
                            self.settings.runtime_dir = path.display().to_string();
                            self.runtime_post_install_prompt = false;
                            self.refresh_runtime_state();
                            self.reset_device_enumeration_cache();
                            self.ensure_devices_enumerated_for_runtime();
                            self.update_setup_modal_after_requirement_change();
                        }
                    }
                });

                ui.horizontal_wrapped(|ui| {
                    let runtime_busy = self.runtime_maintenance_in_progress();
                    let runtime_missing = !self.runtime_check.is_ok();
                    if ui.button("Use default runtime dir").clicked() {
                        self.settings.runtime_dir =
                            self.paths.runtime_shared_dir.display().to_string();
                        self.runtime_post_install_prompt = false;
                        self.refresh_runtime_state();
                        self.reset_device_enumeration_cache();
                        self.ensure_devices_enumerated_for_runtime();
                        self.update_setup_modal_after_requirement_change();
                    }
                    if ui.button("Reload runtime check").clicked() {
                        self.refresh_runtime_state();
                        self.ensure_devices_enumerated_for_runtime();
                        self.update_setup_modal_after_requirement_change();
                    }
                    if ui.button("Reload manifest").clicked() {
                        self.reload_runtime_manifest();
                    }
                    let download_button = if runtime_missing {
                        egui::Button::new(
                            RichText::new("Download / Repair Runtime (Required)")
                                .strong()
                                .color(Color32::WHITE),
                        )
                        .fill(Color32::from_rgb(184, 56, 56))
                        .stroke(Stroke::new(1.0, Color32::from_rgb(138, 35, 35)))
                    } else {
                        egui::Button::new("Download / Repair Runtime")
                    };
                    let mut download_response = ui.add_enabled(!runtime_busy, download_button);
                    if runtime_missing {
                        let tooltip = format!(
                            "Runtime is incomplete. Missing components:\n- {}",
                            self.runtime_check.missing.join("\n- ")
                        );
                        download_response = download_response.on_hover_text(tooltip);
                    }
                    if download_response.clicked() {
                        self.start_runtime_download();
                    }
                    if runtime_unblock_required_for_platform() {
                        if ui
                            .add_enabled(
                                !runtime_busy,
                                egui::Button::new("Unblock unsigned runtime"),
                            )
                            .clicked()
                        {
                            self.start_runtime_unblock();
                        }
                    } else {
                        ui.label("Linux: unsigned-runtime unblock is not required.");
                    }
                });
                if !self.runtime_check.is_ok() {
                    ui.colored_label(
                        Color32::from_rgb(167, 37, 37),
                        "Runtime files are missing. Use 'Download / Repair Runtime (Required)' to install/repair.",
                    );
                    ui.label(
                        RichText::new(format!("- {}", self.runtime_check.missing.join("\n- ")))
                            .small()
                            .color(Color32::from_rgb(167, 37, 37)),
                    );
                }

                if self.runtime_maintenance_in_progress() {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        if self.runtime_download_in_progress {
                            ui.label("Runtime install/repair in progress...");
                        } else if self.runtime_unblock_in_progress {
                            ui.label("Unsigned runtime unblock in progress...");
                        } else {
                            ui.label("Runtime maintenance in progress...");
                        }
                    });
                }
                if self.runtime_post_install_prompt {
                    ui.colored_label(
                        Color32::from_rgb(154, 103, 0),
                        if runtime_unblock_required_for_platform() {
                            "Runtime install finished. Run 'Unblock unsigned runtime' before conversion."
                        } else {
                            "Runtime install finished."
                        },
                    );
                }

                #[cfg(target_os = "windows")]
                {
                    if self.runtime_install_backends.is_empty() {
                        self.runtime_install_backends = Self::default_runtime_backends_for_platform();
                    }
                    let selected_index = self
                        .selected_runtime_install_backend
                        .min(self.runtime_install_backends.len().saturating_sub(1));
                    let selected_text = self
                        .runtime_install_backends
                        .get(selected_index)
                        .cloned()
                        .unwrap_or_else(|| "vulkan".to_owned());
                    let mut next_index = selected_index;
                    ui.horizontal(|ui| {
                        ui.label("Windows runtime backend");
                        egui::ComboBox::from_id_salt("runtime_backend_windows_combo")
                            .selected_text(selected_text.to_ascii_uppercase())
                            .show_ui(ui, |ui| {
                                for (index, backend) in self.runtime_install_backends.iter().enumerate()
                                {
                                    ui.selectable_value(
                                        &mut next_index,
                                        index,
                                        backend.to_ascii_uppercase(),
                                    );
                                }
                            });
                    });
                    if next_index != self.selected_runtime_install_backend {
                        self.selected_runtime_install_backend = next_index;
                        if let Some(backend) = self
                            .runtime_install_backends
                            .get(self.selected_runtime_install_backend)
                        {
                            self.settings.runtime_download_backend = backend.clone();
                            if let Err(err) = self.save_settings_now() {
                                self.runtime_status =
                                    format!("Failed to save runtime backend selection: {err}");
                                self.push_log(LogLevel::Error, self.runtime_status.clone());
                            }
                        }
                    }
                }

                #[cfg(not(target_os = "windows"))]
                {
                    if !self.runtime_assets.is_empty() {
                        let selected_index = self
                            .selected_runtime_asset
                            .min(self.runtime_assets.len().saturating_sub(1));
                        let selected_label =
                            runtime_asset_label(&self.runtime_assets[selected_index]);
                        egui::ComboBox::from_label("Runtime asset")
                            .selected_text(selected_label)
                            .show_ui(ui, |ui| {
                                for (index, asset) in self.runtime_assets.iter().enumerate() {
                                    ui.selectable_value(
                                        &mut self.selected_runtime_asset,
                                        index,
                                        runtime_asset_label(asset),
                                    );
                                }
                            });
                    } else {
                        ui.label("No platform runtime assets loaded.");
                    }
                }

                if self.runtime_check.is_ok() {
                    ui.label("Runtime check: OK");
                } else {
                    ui.label(RichText::new("Runtime check: missing files").strong());
                    for missing in &self.runtime_check.missing {
                        ui.label(format!("- {}", missing));
                    }
                }

                ui.separator();
                ui.heading("PDFVLM / VLM Settings");
                ui.horizontal(|ui| {
                    ui.label("Mode");
                    egui::ComboBox::from_id_salt("settings_conversion_mode")
                        .selected_text(match self.settings.conversion_mode {
                            ConversionMode::FastPdf => "Fast PDF (pdf.dll)",
                            ConversionMode::PdfVlm => "PDF VLM + Image VLM",
                            ConversionMode::FastPdfWithVlmFallback => {
                                "FAST with VLM fallback"
                            }
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.settings.conversion_mode,
                                ConversionMode::FastPdf,
                                "Fast PDF (pdf.dll)",
                            )
                            .on_hover_text(
                                "Machine-readable PDF text extraction via pdf.dll. Fastest path and no VLM model required.",
                            );
                            ui.selectable_value(
                                &mut self.settings.conversion_mode,
                                ConversionMode::PdfVlm,
                                "PDF VLM + Image VLM",
                            )
                            .on_hover_text(
                                "PDFs use PDFVLM. Images are always routed to VLM image mode with your image prompt.",
                            );
                            ui.selectable_value(
                                &mut self.settings.conversion_mode,
                                ConversionMode::FastPdfWithVlmFallback,
                                "FAST with VLM fallback",
                            )
                            .on_hover_text(
                                "Try FAST first; if PDF is not machine-readable, automatically rerun that PDF with VLM.",
                            );
                        });
                });
                ui.add_space(4.0);
                egui::Frame::group(ui.style())
                    .fill(Color32::from_rgb(247, 248, 250))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(216, 220, 227)))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(egui::Margin::same(10))
                    .show(ui, |ui| {
                        ui.label(RichText::new("Models").strong());
                        ui.label(
                            RichText::new(format!(
                                "Model directory: {}",
                                self.paths.models_dir.display()
                            ))
                            .small()
                            .color(Color32::from_rgb(89, 95, 105)),
                        );
                        ui.horizontal(|ui| {
                            ui.label("VLM model");
                            ui.text_edit_singleline(&mut self.settings.vlm_model_path);
                            if ui.button("Pick").clicked() {
                                if let Some(path) = rfd::FileDialog::new()
                                    .set_title("Select VLM model (.gguf)")
                                    .pick_file()
                                {
                                    self.settings.vlm_model_path = path.display().to_string();
                                    match self.save_settings_now() {
                                        Ok(()) => {
                                            self.status_message =
                                                "Saved default VLM model path.".to_owned();
                                            self.update_setup_modal_after_requirement_change();
                                            self.push_log(
                                                LogLevel::Info,
                                                format!(
                                                    "Default VLM model set to '{}'.",
                                                    self.settings.vlm_model_path
                                                ),
                                            );
                                        }
                                        Err(err) => {
                                            self.status_message =
                                                format!("Failed to save model path: {err}");
                                            self.push_log(
                                                LogLevel::Error,
                                                self.status_message.clone(),
                                            );
                                        }
                                    }
                                }
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.label("MMProj model");
                            ui.text_edit_singleline(&mut self.settings.vlm_mmproj_path);
                            if ui.button("Pick").clicked() {
                                if let Some(path) = rfd::FileDialog::new()
                                    .set_title("Select MMProj model (.gguf)")
                                    .pick_file()
                                {
                                    self.settings.vlm_mmproj_path = path.display().to_string();
                                    match self.save_settings_now() {
                                        Ok(()) => {
                                            self.status_message =
                                                "Saved default MMProj model path.".to_owned();
                                            self.update_setup_modal_after_requirement_change();
                                            self.push_log(
                                                LogLevel::Info,
                                                format!(
                                                    "Default MMProj model set to '{}'.",
                                                    self.settings.vlm_mmproj_path
                                                ),
                                            );
                                        }
                                        Err(err) => {
                                            self.status_message =
                                                format!("Failed to save MMProj path: {err}");
                                            self.push_log(
                                                LogLevel::Error,
                                                self.status_message.clone(),
                                            );
                                        }
                                    }
                                }
                            }
                        });
                        self.selected_model_combo_preset = self
                            .selected_model_combo_preset
                            .min(MODEL_COMBO_PRESETS.len().saturating_sub(1));
                        let selected_preset = self.current_model_combo_preset();
                        ui.horizontal_wrapped(|ui| {
                            ui.label("Suggested combo");
                            egui::ComboBox::from_id_salt("model_combo_preset")
                                .selected_text(format!(
                                    "{} ({})",
                                    selected_preset.label, selected_preset.requirement
                                ))
                                .show_ui(ui, |ui| {
                                    for (index, preset) in MODEL_COMBO_PRESETS.iter().enumerate() {
                                        ui.selectable_value(
                                            &mut self.selected_model_combo_preset,
                                            index,
                                            format!(
                                                "{} ({})",
                                                preset.label, preset.requirement
                                            ),
                                        );
                                    }
                                });
                        });
                        let selected_preset = self.current_model_combo_preset();
                        ui.horizontal_wrapped(|ui| {
                            if ui
                                .add_enabled(
                                    !self.model_download_in_progress,
                                    egui::Button::new("Download Selected Model + MMProj"),
                                )
                                .clicked()
                            {
                                self.start_selected_model_combo_download();
                            }
                        });
                        ui.hyperlink_to("Selected combo repo (Hugging Face)", selected_preset.repo_url);
                        ui.label(
                            RichText::new(format!(
                                "Estimated memory requirement: {}",
                                selected_preset.requirement
                            ))
                            .small()
                            .color(Color32::from_rgb(89, 95, 105)),
                        );
                        ui.label(
                            RichText::new(format!("Model file: {}", selected_preset.model_file))
                                .small()
                                .color(Color32::from_rgb(89, 95, 105)),
                        );
                        ui.label(
                            RichText::new(format!("MMProj file: {}", selected_preset.mmproj_file))
                                .small()
                                .color(Color32::from_rgb(89, 95, 105)),
                        );
                        ui.label(
                            RichText::new(selected_preset.notes)
                                .small()
                                .color(Color32::from_rgb(89, 95, 105)),
                        );
                        ui.label(
                            RichText::new(format!(
                                "Selected combo downloads are saved to: {}",
                                self.paths.models_dir.display()
                            ))
                            .small()
                            .color(Color32::from_rgb(89, 95, 105)),
                        );
                        ui.label(
                            RichText::new("MMProj is the vision projector GGUF. VLM models require both main model GGUF and matching MMProj GGUF.")
                                .small()
                                .color(Color32::from_rgb(89, 95, 105)),
                        );
                        ui.label(
                            RichText::new("If the app crashes during VLM runs, lower n_ctx and n_parallel (and optionally batch values) in Runtime params below.")
                                .small()
                                .color(Color32::from_rgb(89, 95, 105)),
                        );
                        ui.label(
                            RichText::new("You can manually load any vision-capable GGUF + MMProj pair supported by llama.cpp using the file pickers above.")
                                .small()
                                .color(Color32::from_rgb(89, 95, 105)),
                        );
                        ui.label(
                            RichText::new("This application is not affiliated with, endorsed by, or sponsored by Qwen, Hugging Face, or llama.cpp.")
                                .small()
                                .color(Color32::from_rgb(89, 95, 105)),
                        );
                    });

                ui.horizontal_wrapped(|ui| {
                    ui.label("Prompt overrides");
                    if ui.button("Edit PDF/Image prompts").clicked() {
                        self.prompt_overrides_window_open = true;
                    }
                    if ui.button("Restore prompt defaults").clicked() {
                        self.settings.vlm_prompt = DEFAULT_VLM_PROMPT.to_owned();
                        self.settings.vlm_image_prompt = DEFAULT_IMAGE_VLM_PROMPT.to_owned();
                    }
                });
                ui.label(
                    RichText::new("PDF and image prompts are configurable from the Prompts window and saved in your app settings JSON.")
                        .small()
                        .color(Color32::from_rgb(89, 95, 105)),
                );

                ui.separator();
                ui.heading("GPU / CPU");
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add_enabled(
                            !self.device_enumeration_in_progress,
                            egui::Button::new("Enumerate runtime devices"),
                        )
                        .clicked()
                    {
                        self.start_device_enumeration();
                    }
                    if self.device_enumeration_in_progress {
                        ui.label("Enumerating devices...");
                    } else {
                        ui.label(format!(
                            "{} runtime device(s), {} selectable option(s)",
                            self.available_devices.len(),
                            self.device_options.len()
                        ));
                    }
                });

                if self.device_options.is_empty() {
                    self.rebuild_device_options_from_available();
                }

                let selected_label = self
                    .device_options
                    .get(self.selected_device_option)
                    .map(|opt| opt.label.clone())
                    .unwrap_or_else(|| VLM_DEVICE_CPU_LABEL.to_owned());
                let previous_device_index = self.selected_device_option;
                ui.horizontal_wrapped(|ui| {
                    ui.label("Execution device");
                    egui::ComboBox::from_id_salt("vlm_execution_device")
                        .selected_text(selected_label)
                        .show_ui(ui, |ui| {
                            for (index, option) in self.device_options.iter().enumerate() {
                                ui.selectable_value(
                                    &mut self.selected_device_option,
                                    index,
                                    option.label.as_str(),
                                );
                            }
                        });
                });
                if self.selected_device_option != previous_device_index {
                    self.apply_selected_device_to_settings();
                    let selected_label = self
                        .device_options
                        .get(self.selected_device_option)
                        .map(|opt| opt.label.clone())
                        .unwrap_or_else(|| VLM_DEVICE_CPU_LABEL.to_owned());
                    match self.save_settings_now() {
                        Ok(()) => {
                            self.status_message =
                                format!("Execution device set to {selected_label}.");
                            self.push_log(LogLevel::Info, self.status_message.clone());
                        }
                        Err(err) => {
                            self.status_message =
                                format!("Failed to save execution device: {err}");
                            self.push_log(LogLevel::Error, self.status_message.clone());
                        }
                    }
                }

                let selected_is_gpu = self
                    .device_options
                    .get(self.selected_device_option)
                    .map(|option| option.is_gpu)
                    .unwrap_or(false);
                if selected_is_gpu {
                    ui.label("GPU selected: runtime uses a single-device GPU selector and keeps text model + mmproj together.");
                } else {
                    ui.label("CPU selected: runtime call forces CPU mode (devices=none).");
                }

                ui.horizontal_wrapped(|ui| {
                    ui.label("n_predict");
                    ui.add(egui::DragValue::new(&mut self.settings.n_predict).speed(10));
                    ui.label("n_ctx");
                    ui.add(egui::DragValue::new(&mut self.settings.n_ctx).speed(128));
                    ui.label("n_batch");
                    ui.add(egui::DragValue::new(&mut self.settings.n_batch).speed(64));
                    ui.label("n_ubatch");
                    ui.add(egui::DragValue::new(&mut self.settings.n_ubatch).speed(64));
                    ui.label("n_parallel");
                    ui.add(egui::DragValue::new(&mut self.settings.n_parallel).speed(1));
                });
                if !selected_is_gpu {
                    ui.horizontal_wrapped(|ui| {
                        ui.label("n_threads");
                        ui.add(egui::DragValue::new(&mut self.settings.n_threads).speed(1));
                        ui.label("n_threads_batch");
                        ui.add(egui::DragValue::new(&mut self.settings.n_threads_batch).speed(1));
                    });
                    ui.label(
                        RichText::new(
                            "CPU thread controls are only used in CPU mode. Keep 0 for automatic thread count.",
                        )
                        .small()
                        .color(Color32::from_rgb(89, 95, 105)),
                    );
                }

                ui.separator();
                if ui.button("Save settings now").clicked() {
                    match self.save_settings_now() {
                        Ok(()) => {
                            self.status_message = "Settings saved.".to_owned();
                            self.refresh_runtime_state();
                            self.update_setup_modal_after_requirement_change();
                        }
                        Err(err) => self.status_message = format!("Failed to save settings: {err}"),
                    }
                }
            });

        if dismiss_setup_popup || !open {
            self.runtime_popup_open = false;
        } else if setup_required {
            self.runtime_popup_open = self.has_missing_essentials();
        }
        self.settings_window_open = open;
    }

    fn ui_bottom_job_bar(&mut self, ctx: &egui::Context) {
        let (pending, running, completed, failed) = self.job_counts();
        let summary = format!(
            "Jobs  Pending: {pending}   Running: {running}   Completed: {completed}   Failed: {failed}   |   {}",
            self.active_job_summary()
        );

        egui::TopBottomPanel::bottom("bottom_job_bar")
            .resizable(false)
            .frame(
                egui::Frame::default()
                    .fill(Color32::from_rgb(235, 238, 242))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(198, 204, 213)))
                    .inner_margin(egui::Margin::symmetric(10, 8)),
            )
            .show(ctx, |ui| {
                let response = ui.add_sized(
                    [ui.available_width(), 28.0],
                    egui::Button::new(
                        RichText::new(summary)
                            .small()
                            .color(Color32::from_rgb(38, 43, 50)),
                    )
                    .fill(Color32::from_rgb(225, 230, 236))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(182, 191, 202))),
                );
                if response.clicked() {
                    self.logs_window_open = true;
                }
                response.on_hover_text("Click to open Jobs / Logs.");
            });
    }

    fn ui_logs_window(&mut self, ctx: &egui::Context) {
        if !self.logs_window_open {
            return;
        }

        let viewport_id = egui::ViewportId::from_hash_of("jobs_logs_viewport");
        let mut should_close = false;
        let (pending, running, completed, failed) = self.job_counts();

        ctx.show_viewport_immediate(
            viewport_id,
            with_app_icon(
                egui::ViewportBuilder::default()
                    .with_title("Jobs & Logs - PDF Markdown Studio")
                    .with_inner_size([1000.0, 620.0])
                    .with_min_inner_size([760.0, 420.0]),
            ),
            |ctx, _class| {
                if ctx.input(|input| input.viewport().close_requested()) {
                    should_close = true;
                    return;
                }

                egui::TopBottomPanel::top("logs_header_panel")
                    .resizable(false)
                    .show(ctx, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new(format!(
                                    "Pending: {pending}  |  Running: {running}  |  Completed: {completed}  |  Failed: {failed}"
                                ))
                                .strong(),
                            );
                        });
                    });

                egui::TopBottomPanel::bottom("logs_footer_panel")
                    .resizable(false)
                    .show(ctx, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            if ui.button("Clear Completed/Failed").clicked() {
                                self.jobs.retain(|job| {
                                    matches!(job.state, JobState::Pending | JobState::Running)
                                });
                            }
                            if ui.button("Clear Logs").clicked() {
                                self.log_entries.clear();
                            }
                            ui.separator();
                            if ui.button("Close").clicked() {
                                should_close = true;
                            }
                        });
                    });

                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.label(RichText::new("Job Queue").strong());
                    ScrollArea::vertical()
                        .id_salt("jobs_scroll_standalone")
                        .max_height(220.0)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            if self.jobs.is_empty() {
                                ui.label("No jobs yet.");
                            } else {
                                for job in self.jobs.iter().rev() {
                                    let state_color = match job.state {
                                        JobState::Pending => Color32::from_rgb(123, 94, 14),
                                        JobState::Running => Color32::from_rgb(29, 97, 160),
                                        JobState::Completed => Color32::from_rgb(27, 121, 72),
                                        JobState::Failed => Color32::from_rgb(165, 43, 43),
                                    };
                                    let state_label = match job.state {
                                        JobState::Pending => "PENDING",
                                        JobState::Running => "RUNNING",
                                        JobState::Completed => "COMPLETED",
                                        JobState::Failed => "FAILED",
                                    };
                                    let percent_suffix = job
                                        .progress_percent
                                        .map(|value| format!(" ({value:.1}%)"))
                                        .unwrap_or_default();
                                    ui.label(
                                        RichText::new(format!(
                                            "#{:04} [{}] [{}] {}{}",
                                            job.id,
                                            job.kind.label(),
                                            state_label,
                                            job.title,
                                            percent_suffix
                                        ))
                                        .color(state_color)
                                        .strong(),
                                    );
                                    ui.label(
                                        RichText::new(job.detail.clone())
                                            .small()
                                            .color(Color32::from_rgb(83, 89, 98)),
                                    );
                                    ui.add_space(4.0);
                                }
                            }
                        });

                    ui.separator();
                    ui.label(RichText::new("Log Stream").strong());
                    ScrollArea::vertical()
                        .id_salt("log_stream_scroll_standalone")
                        .auto_shrink([false, false])
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            if self.log_entries.is_empty() {
                                ui.label("No logs yet.");
                                return;
                            }

                            for entry in &self.log_entries {
                                let level = match entry.level {
                                    LogLevel::Info => "INFO",
                                    LogLevel::Warn => "WARN",
                                    LogLevel::Error => "ERROR",
                                };
                                let color = match entry.level {
                                    LogLevel::Info => Color32::from_rgb(66, 72, 82),
                                    LogLevel::Warn => Color32::from_rgb(142, 103, 28),
                                    LogLevel::Error => Color32::from_rgb(158, 49, 49),
                                };
                                ui.monospace(
                                    RichText::new(format!(
                                        "[{}] [{}] {}",
                                        entry.timestamp,
                                        level,
                                        entry.message
                                    ))
                                    .color(color),
                                );
                            }
                        });
                });
            },
        );

        if should_close {
            self.logs_window_open = false;
        }
    }

    fn ui_central_panel(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(
                egui::Frame::default()
                    .fill(Color32::from_rgb(242, 243, 245))
                    .inner_margin(egui::Margin::same(12)),
            )
            .show(ctx, |ui| {
                egui::Frame::group(ui.style())
                    .fill(Color32::from_rgb(250, 250, 251))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(219, 221, 226)))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(egui::Margin::same(10))
                    .show(ui, |ui| {
                        self.ui_search_bar(ui);
                    });
                ui.add_space(10.0);

                let Some(selected_index) = self.selected_doc_index() else {
                    ui.centered_and_justified(|ui| {
                        ui.label("No document selected.");
                    });
                    return;
                };
                let split_viewport_width = ui.available_width().max(1.0);
                let split_viewport_height = (ui.available_height() - 30.0).max(240.0);

                ScrollArea::both()
                    .id_salt("split_view_outer_scroll")
                    .auto_shrink([false, false])
                    .max_height(split_viewport_height)
                    .min_scrolled_width(split_viewport_width)
                    .show(ui, |ui| {
                        self.ui_split_view(
                            ui,
                            ctx,
                            selected_index,
                            split_viewport_width,
                            split_viewport_height,
                        );
                    });
                ui.add_space(10.0);
                ui.label(
                    RichText::new(&self.status_message)
                        .italics()
                        .color(Color32::from_rgb(94, 99, 108)),
                );
            });
    }
}

impl eframe::App for PdfMarkdownApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.process_background_events(ctx);
        self.ui_menu_bar(ctx);
        self.ui_top_panel(ctx);
        self.ui_bottom_job_bar(ctx);
        self.ui_documents_sidebar(ctx);
        self.ui_central_panel(ctx);
        self.ui_markdown_unsaved_dialog(ctx);
        self.ui_prompt_overrides_window(ctx);
        self.ui_about_window(ctx);
        self.ui_legal_docs_window(ctx);
        self.ui_settings_window(ctx);
        self.ui_logs_window(ctx);

        if self.has_background_work() {
            ctx.request_repaint_after(self.background_repaint_interval());
        }
    }
}

fn unsigned_runtime_script_file_name() -> &'static str {
    if cfg!(windows) {
        "unblock-unsigned-runtime.ps1"
    } else {
        "unblock-unsigned-runtime.sh"
    }
}

fn runtime_unblock_required_for_platform() -> bool {
    cfg!(windows) || cfg!(target_os = "macos")
}

fn bundled_unsigned_runtime_script_contents() -> &'static str {
    if cfg!(windows) {
        BUNDLED_UNBLOCK_UNSIGNED_RUNTIME_PS1
    } else {
        BUNDLED_UNBLOCK_UNSIGNED_RUNTIME_SH
    }
}

fn materialize_bundled_unsigned_runtime_script(paths: &AppPaths) -> Result<PathBuf, String> {
    let script_dir = paths.app_config_dir.join("runtime-scripts");
    std::fs::create_dir_all(&script_dir).map_err(|err| {
        format!(
            "failed to create runtime script directory '{}': {err}",
            script_dir.display()
        )
    })?;

    let script_path = script_dir.join(unsigned_runtime_script_file_name());
    std::fs::write(
        &script_path,
        bundled_unsigned_runtime_script_contents().as_bytes(),
    )
    .map_err(|err| {
        format!(
            "failed to write bundled unsigned-runtime script '{}': {err}",
            script_path.display()
        )
    })?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&script_path)
            .map_err(|err| {
                format!(
                    "failed to read script metadata '{}': {err}",
                    script_path.display()
                )
            })?
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script_path, permissions).map_err(|err| {
            format!(
                "failed to set script permissions '{}': {err}",
                script_path.display()
            )
        })?;
    }

    Ok(script_path)
}

fn run_unsigned_runtime_unblock_script(
    paths: &AppPaths,
    runtime_dir: &Path,
) -> Result<String, String> {
    if !runtime_unblock_required_for_platform() {
        return Ok("Unsigned runtime unblock is not required on Linux.".to_owned());
    }

    if !runtime_dir.exists() {
        return Err(format!(
            "Runtime directory does not exist: '{}'",
            runtime_dir.display()
        ));
    }

    let script_path = materialize_bundled_unsigned_runtime_script(paths)?;

    let mut command = if cfg!(windows) {
        let mut cmd = Command::new("powershell");
        cmd.arg("-NoProfile")
            .arg("-ExecutionPolicy")
            .arg("Bypass")
            .arg("-File")
            .arg(&script_path)
            .arg("-RuntimeDir")
            .arg(runtime_dir);
        cmd
    } else {
        let mut cmd = Command::new("sh");
        cmd.arg(&script_path).arg(runtime_dir);
        cmd
    };
    command.stdin(Stdio::null());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());

    let output = command.output().map_err(|err| {
        format!(
            "failed to execute unsigned-runtime script '{}': {err}",
            script_path.display()
        )
    })?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !output.status.success() {
        let details = if stderr.is_empty() { stdout } else { stderr };
        return Err(format!(
            "unsigned-runtime script failed ({}): {}",
            script_path.display(),
            details
        ));
    }

    let mut message = format!("Unsigned runtime script applied: {}", script_path.display());
    if !stdout.is_empty() {
        message.push_str(&format!(" | {stdout}"));
    }
    Ok(message)
}

fn native_menu_item(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.add(egui::Button::new(label).wrap_mode(egui::TextWrapMode::Extend))
}

fn apply_modern_theme(ctx: &egui::Context) {
    #[cfg(windows)]
    {
        let mut fonts = FontDefinitions::default();
        let mut updated = false;

        if let Ok(bytes) = std::fs::read(r"C:\Windows\Fonts\segoeui.ttf") {
            fonts
                .font_data
                .insert("segoe_ui".into(), FontData::from_owned(bytes).into());
            if let Some(family) = fonts.families.get_mut(&FontFamily::Proportional) {
                family.insert(0, "segoe_ui".into());
            }
            updated = true;
        }

        if let Ok(bytes) = std::fs::read(r"C:\Windows\Fonts\consola.ttf") {
            fonts
                .font_data
                .insert("consolas".into(), FontData::from_owned(bytes).into());
            if let Some(family) = fonts.families.get_mut(&FontFamily::Monospace) {
                family.insert(0, "consolas".into());
            }
            updated = true;
        }

        if updated {
            ctx.set_fonts(fonts);
        }
    }

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(10.0, 8.0);
    style.spacing.button_padding = egui::vec2(10.0, 7.0);
    style.spacing.interact_size = egui::vec2(46.0, 30.0);
    style.spacing.window_margin = egui::Margin::same(10);
    style.spacing.menu_margin = egui::Margin::same(8);
    style.spacing.scroll = egui::style::ScrollStyle::solid();

    style.text_styles.insert(
        egui::TextStyle::Heading,
        FontId::new(23.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Body,
        FontId::new(16.5, FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Monospace,
        FontId::new(15.5, FontFamily::Monospace),
    );
    style.text_styles.insert(
        egui::TextStyle::Button,
        FontId::new(15.5, FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Small,
        FontId::new(14.0, FontFamily::Proportional),
    );

    style.visuals = modern_visuals();
    ctx.set_style(style);
}

fn modern_visuals() -> egui::Visuals {
    let mut visuals = egui::Visuals::light();
    visuals.override_text_color = Some(Color32::from_rgb(36, 39, 44));
    visuals.panel_fill = Color32::from_rgb(242, 243, 246);
    visuals.window_fill = Color32::from_rgb(252, 252, 253);
    visuals.faint_bg_color = Color32::from_rgb(246, 247, 249);
    visuals.extreme_bg_color = Color32::from_rgb(233, 235, 239);
    visuals.code_bg_color = Color32::from_rgb(238, 240, 243);
    visuals.window_stroke = Stroke::new(1.0, Color32::from_rgb(212, 215, 221));
    visuals.window_corner_radius = CornerRadius::same(12);
    visuals.menu_corner_radius = CornerRadius::same(10);
    visuals.hyperlink_color = Color32::from_rgb(42, 122, 92);

    visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(248, 248, 250);
    visuals.widgets.noninteractive.weak_bg_fill = Color32::from_rgb(248, 248, 250);
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(216, 219, 225));
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, Color32::from_rgb(86, 91, 100));
    visuals.widgets.noninteractive.corner_radius = CornerRadius::same(8);

    visuals.widgets.inactive.bg_fill = Color32::from_rgb(238, 240, 244);
    visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(238, 240, 244);
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(196, 201, 210));
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, Color32::from_rgb(64, 68, 76));
    visuals.widgets.inactive.corner_radius = CornerRadius::same(8);

    visuals.widgets.hovered.bg_fill = Color32::from_rgb(229, 236, 232);
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(229, 236, 232);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(152, 174, 162));
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.2, Color32::from_rgb(36, 92, 69));
    visuals.widgets.hovered.corner_radius = CornerRadius::same(8);

    visuals.widgets.active.bg_fill = Color32::from_rgb(214, 231, 223);
    visuals.widgets.active.weak_bg_fill = Color32::from_rgb(214, 231, 223);
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, Color32::from_rgb(106, 160, 133));
    visuals.widgets.active.fg_stroke = Stroke::new(1.2, Color32::from_rgb(31, 88, 64));
    visuals.widgets.active.corner_radius = CornerRadius::same(8);

    visuals.widgets.open.bg_fill = Color32::from_rgb(225, 236, 230);
    visuals.widgets.open.weak_bg_fill = Color32::from_rgb(225, 236, 230);
    visuals.widgets.open.bg_stroke = Stroke::new(1.0, Color32::from_rgb(129, 166, 147));
    visuals.widgets.open.fg_stroke = Stroke::new(1.1, Color32::from_rgb(36, 90, 68));
    visuals.widgets.open.corner_radius = CornerRadius::same(8);

    visuals.selection.bg_fill = Color32::from_rgb(38, 128, 95);
    visuals.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    visuals
}

fn format_log_timestamp_now() -> String {
    Local::now().format("%Y-%m-%d %H:%M:%S%.3f %:z").to_string()
}

#[cfg(not(target_os = "windows"))]
fn runtime_asset_label(asset: &ManifestAsset) -> String {
    let platform = if asset.platform.trim().is_empty() {
        "unknown-platform"
    } else {
        asset.platform.trim()
    };
    let backend = if asset.backend.trim().is_empty() {
        "unknown-backend"
    } else {
        asset.backend.trim()
    };
    let identity = if !asset.id.trim().is_empty() {
        asset.id.trim()
    } else if !asset.file_name.trim().is_empty() {
        asset.file_name.trim()
    } else {
        "runtime-asset"
    };
    format!("{platform} | {backend} | {identity}")
}

fn resolve_active_page_by_rect_visibility(
    page_rects: &[egui::Rect],
    viewport_rect: egui::Rect,
) -> Option<usize> {
    if page_rects.is_empty() {
        return None;
    }

    let mut threshold_match: Option<(usize, f32)> = None;
    let mut best_visible_match: Option<(usize, f32)> = None;

    for (index, rect) in page_rects.iter().enumerate() {
        let height = rect.height().max(1.0);
        let visible = (rect.bottom().min(viewport_rect.bottom())
            - rect.top().max(viewport_rect.top()))
        .max(0.0);
        let ratio = (visible / height).clamp(0.0, 1.0);
        match best_visible_match {
            Some((best_index, best_ratio))
                if ratio < best_ratio || (ratio == best_ratio && index <= best_index) => {}
            _ => {
                best_visible_match = Some((index, ratio));
            }
        }
        if ratio >= PAGE_ACTIVE_VISIBILITY_THRESHOLD {
            match threshold_match {
                Some((best_index, best_ratio))
                    if ratio < best_ratio || (ratio == best_ratio && index <= best_index) => {}
                _ => {
                    threshold_match = Some((index, ratio));
                }
            }
        }
    }

    if let Some((index, _)) = threshold_match {
        return Some(index);
    }
    if let Some((index, _)) = best_visible_match {
        return Some(index);
    }

    Some(0)
}

fn render_source_pane(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    document: &WorkspaceDocument,
    current_page: usize,
    pending_sync_to_pdf: &mut Option<usize>,
) -> PaneMetrics {
    ui.heading("Original");

    match &document.kind {
        DocumentKind::Pdf(pdf_data) => {
            let scroll_area = ScrollArea::vertical()
                .id_salt(("source_pdf", document.id))
                .auto_shrink([false, false]);

            let scroll_output = scroll_area.show(ui, |ui| {
                let mut page_rects = Vec::with_capacity(pdf_data.pages.len());
                for (index, page) in pdf_data.pages.iter().enumerate() {
                    let highlight_stroke = if current_page == index {
                        Stroke::new(2.0, ui.visuals().selection.stroke.color)
                    } else {
                        Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color)
                    };

                    let frame_response = egui::Frame::group(ui.style())
                        .stroke(highlight_stroke)
                        .inner_margin(egui::Margin::symmetric(8, 8))
                        .show(ui, |ui| {
                            // Fit to the frame's inner width so the frame never overflows into the sibling pane.
                            let fit_width = ui.available_width().max(1.0);
                            ui.label(RichText::new(format!("Page {}", index + 1)).strong());
                            let fit_scale = fit_width / page.image_size.x.max(1.0);
                            let display_size = page.image_size * fit_scale;
                            ui.add(egui::Image::new(&page.texture).fit_to_exact_size(display_size));
                        })
                        .response;
                    page_rects.push(frame_response.rect);

                    if pending_sync_to_pdf.is_some_and(|target| target == index) {
                        ui.scroll_to_rect(frame_response.rect, Some(Align::TOP));
                        *pending_sync_to_pdf = None;
                    }

                    ui.add_space(6.0);
                }
                page_rects
            });

            let hovered = ctx
                .pointer_hover_pos()
                .is_some_and(|position| scroll_output.inner_rect.contains(position));
            let user_scrolled = hovered
                && ctx.input(|input| {
                    input.raw_scroll_delta.y.abs() > f32::EPSILON
                        || input.smooth_scroll_delta.y.abs() > f32::EPSILON
                });
            let vertical_scroll_offset = scroll_output.state.offset.y.max(0.0);
            let first_visible_page = resolve_active_page_by_rect_visibility(
                &scroll_output.inner,
                scroll_output.inner_rect,
            );

            PaneMetrics {
                hovered,
                user_scrolled,
                scroll_offset_y: vertical_scroll_offset,
                first_visible_page,
            }
        }
        DocumentKind::Image(image_data) => {
            let scroll_output = ScrollArea::vertical()
                .id_salt(("source_image", document.id))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let fit_width = ui.available_width().max(120.0);
                    let fit_scale = fit_width / image_data.image_size.x.max(1.0);
                    let display_size = image_data.image_size * fit_scale;
                    ui.add(egui::Image::new(&image_data.texture).fit_to_exact_size(display_size));
                });

            let hovered = ctx
                .pointer_hover_pos()
                .is_some_and(|position| scroll_output.inner_rect.contains(position));
            let user_scrolled = hovered
                && ctx.input(|input| {
                    input.raw_scroll_delta.y.abs() > f32::EPSILON
                        || input.smooth_scroll_delta.y.abs() > f32::EPSILON
                });
            let vertical_scroll_offset = scroll_output.state.offset.y.max(0.0);
            PaneMetrics {
                hovered,
                user_scrolled,
                scroll_offset_y: vertical_scroll_offset,
                first_visible_page: Some(0),
            }
        }
    }
}

fn render_markdown_pane(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    document: &mut WorkspaceDocument,
    current_page: usize,
    markdown_font_size: f32,
    page_height_targets: &[f32],
    enforce_page_height_sync: bool,
    pending_edit_focus_page: &mut Option<usize>,
    last_toggle_at: &mut f64,
    markdown_cache: &mut CommonMarkCache,
    pending_sync_to_markdown: &mut Option<usize>,
    ui_actions: &mut MarkdownPaneUiActions,
) -> PaneMetrics {
    ui.heading("Markdown");

    let mut markdown_style = ui.style().as_ref().clone();
    markdown_style.text_styles.insert(
        egui::TextStyle::Body,
        FontId::proportional(markdown_font_size.max(8.0)),
    );
    markdown_style.text_styles.insert(
        egui::TextStyle::Monospace,
        FontId::monospace((markdown_font_size - 1.0).max(8.0)),
    );
    markdown_style.spacing.item_spacing.y = 6.0;

    if document.markdown_edit_mode {
        let dirty = PdfMarkdownApp::document_markdown_is_dirty(document);
        ui.horizontal_wrapped(|ui| {
            if dirty
                && ui
                    .add(
                        egui::Button::new(RichText::new("Save Edits").color(Color32::WHITE))
                            .fill(Color32::from_rgb(35, 121, 90))
                            .stroke(Stroke::new(1.0, Color32::from_rgb(28, 94, 70))),
                    )
                    .clicked()
            {
                ui_actions.request_save_edits = true;
            }
            if dirty {
                ui.label(
                    RichText::new("Unsaved changes")
                        .small()
                        .color(Color32::from_rgb(130, 86, 38)),
                );
            } else {
                ui.label(
                    RichText::new("Editing mode")
                        .small()
                        .color(Color32::from_rgb(96, 101, 111)),
                );
            }
            if ui.button("Close Editing").clicked() {
                ui_actions.request_exit_edit_mode = true;
            }
        });
        ui.add_space(6.0);

        let scroll_output = ui
            .scope(|ui| {
                ui.set_style(markdown_style);
                ScrollArea::both()
                    .id_salt(("md_view", document.id))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let page_width = ui.clip_rect().width().max(1.0);
                        let edit_id = ui.make_persistent_id(("md_edit_text", document.id));
                        let mut should_focus_editor = false;

                        if let Some(page_to_focus) = pending_edit_focus_page.take() {
                            let target_char_index =
                                markdown_char_index_for_page(&document.markdown, page_to_focus);
                            let mut state = egui::text_edit::TextEditState::load(ui.ctx(), edit_id)
                                .unwrap_or_default();
                            state
                                .cursor
                                .set_char_range(Some(egui::text::CCursorRange::one(
                                    egui::text::CCursor::new(target_char_index),
                                )));
                            state.store(ui.ctx(), edit_id);
                            should_focus_editor = true;
                        }

                        let output = egui::TextEdit::multiline(&mut document.markdown)
                            .id(edit_id)
                            .desired_width(page_width)
                            .desired_rows(50)
                            .code_editor()
                            .show(ui);
                        if should_focus_editor {
                            output.response.request_focus();
                        }
                    })
            })
            .inner;

        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if dirty
                && ui
                    .add(
                        egui::Button::new(RichText::new("Save Edits").color(Color32::WHITE))
                            .fill(Color32::from_rgb(35, 121, 90))
                            .stroke(Stroke::new(1.0, Color32::from_rgb(28, 94, 70))),
                    )
                    .clicked()
            {
                ui_actions.request_save_edits = true;
            }
            if ui.button("Close Editing").clicked() {
                ui_actions.request_exit_edit_mode = true;
            }
        });

        let hovered = ctx
            .pointer_hover_pos()
            .is_some_and(|position| scroll_output.inner_rect.contains(position));
        let user_scrolled = hovered
            && ctx.input(|input| {
                input.raw_scroll_delta.y.abs() > f32::EPSILON
                    || input.smooth_scroll_delta.y.abs() > f32::EPSILON
            });
        let vertical_scroll_offset = scroll_output.state.offset.y.max(0.0);
        let double_clicked = hovered
            && ctx.input(|input| {
                input
                    .pointer
                    .button_double_clicked(egui::PointerButton::Primary)
            });
        let now = ctx.input(|input| input.time);
        if double_clicked && (now - *last_toggle_at) > 0.28 {
            ui_actions.request_exit_edit_mode = true;
            *last_toggle_at = now;
        }

        return PaneMetrics {
            hovered,
            user_scrolled,
            scroll_offset_y: vertical_scroll_offset,
            first_visible_page: Some(current_page),
        };
    }

    let page_count = document.page_count();
    let markdown_pages = split_markdown_by_page_markers(&document.markdown, page_count);

    let scroll_area = ScrollArea::vertical()
        .id_salt(("md_view", document.id))
        .max_width(ui.available_width().max(1.0))
        .auto_shrink([false, false]);

    let scroll_output = ui
        .scope(|ui| {
            ui.set_style(markdown_style);
            scroll_area.show(ui, |ui| {
                let page_outer_width = ui.available_width().max(1.0);
                ui.set_max_width(page_outer_width);
                let mut page_rects = Vec::with_capacity(page_count);

                for page_index in 0..page_count {
                    let current_page_stroke = if current_page == page_index {
                        Stroke::new(2.0, ui.visuals().selection.stroke.color)
                    } else {
                        Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color)
                    };
                    let target_height = if enforce_page_height_sync {
                        page_height_targets
                            .get(page_index)
                            .copied()
                            .unwrap_or_default()
                            .max(0.0)
                    } else {
                        0.0
                    };

                    let content = markdown_pages
                        .get(page_index)
                        .map_or("", std::string::String::as_str);
                    // Frame has 8px inner margin on each side, so reserve that space up-front
                    // to keep the outer frame fully inside the pane.
                    let render_width = (page_outer_width - 16.0).max(1.0);
                    let frame_response = egui::Frame::group(ui.style())
                        .stroke(current_page_stroke)
                        .inner_margin(egui::Margin::symmetric(8, 8))
                        .show(ui, |ui| {
                            ui.set_min_width(render_width);
                            ui.set_max_width(render_width);
                            if enforce_page_height_sync && target_height > 0.0 {
                                ui.set_min_height((target_height - 16.0).max(0.0));
                            }
                            ui.monospace(format!("<--page{}-->", page_index + 1));
                            ui.separator();

                            ui.push_id(("md_page", document.id, page_index), |ui| {
                                let render_blocks = split_markdown_render_blocks(content);
                                let mut render_blocks_ui = |ui: &mut egui::Ui| {
                                    for (block_index, block) in render_blocks.iter().enumerate() {
                                        match block {
                                            MarkdownRenderBlock::Markdown(markdown) => {
                                                if markdown.trim().is_empty() {
                                                    continue;
                                                }
                                                ui.push_id(
                                                    (
                                                        "md_page_text",
                                                        document.id,
                                                        page_index,
                                                        block_index,
                                                    ),
                                                    |ui| {
                                                        ui.set_max_width(render_width);
                                                        CommonMarkViewer::new()
                                                            .default_width(Some(
                                                                render_width as usize,
                                                            ))
                                                            .show(ui, markdown_cache, markdown);
                                                    },
                                                );
                                            }
                                            MarkdownRenderBlock::Table { rows, alignments } => {
                                                if rows.is_empty() {
                                                    continue;
                                                }
                                                render_markdown_table_grid(
                                                    ui,
                                                    rows,
                                                    alignments,
                                                    markdown_font_size,
                                                    document.id,
                                                    page_index,
                                                    block_index,
                                                );
                                            }
                                        }
                                        if block_index + 1 < render_blocks.len() {
                                            ui.add_space(2.0);
                                        }
                                    }
                                };
                                render_blocks_ui(ui);
                            });
                        })
                        .response;
                    page_rects.push(frame_response.rect);

                    if pending_sync_to_markdown.is_some_and(|target| target == page_index) {
                        ui.scroll_to_rect(frame_response.rect, Some(Align::TOP));
                        *pending_sync_to_markdown = None;
                    }

                    ui.add_space(6.0);
                }

                page_rects
            })
        })
        .inner;

    let hovered = ctx
        .pointer_hover_pos()
        .is_some_and(|position| scroll_output.inner_rect.contains(position));
    let user_scrolled = hovered
        && ctx.input(|input| {
            input.raw_scroll_delta.y.abs() > f32::EPSILON
                || input.smooth_scroll_delta.y.abs() > f32::EPSILON
        });
    let vertical_scroll_offset = scroll_output.state.offset.y.max(0.0);
    let first_visible_page =
        resolve_active_page_by_rect_visibility(&scroll_output.inner, scroll_output.inner_rect);
    let double_clicked = hovered
        && ctx.input(|input| {
            input
                .pointer
                .button_double_clicked(egui::PointerButton::Primary)
        });
    let now = ctx.input(|input| input.time);
    if double_clicked && (now - *last_toggle_at) > 0.28 {
        ui_actions.request_enter_edit_mode = true;
        ui_actions.request_enter_edit_mode_page = first_visible_page.or(Some(current_page));
        *last_toggle_at = now;
    }

    PaneMetrics {
        hovered,
        user_scrolled,
        scroll_offset_y: vertical_scroll_offset,
        first_visible_page,
    }
}

fn resolve_pdfium_library_path(runtime_dir: &Path) -> Option<PathBuf> {
    let runtime_pdfium = runtime_pdfium_library_path(runtime_dir);
    let candidates = vec![runtime_pdfium, runtime_dir.join("vendor").join("pdfium")];

    for candidate in candidates {
        let resolved = if candidate.is_dir() {
            Pdfium::pdfium_platform_library_name_at_path(&candidate)
        } else {
            candidate
        };
        if resolved.exists() {
            return Some(resolved);
        }
    }

    None
}

fn bind_pdfium_for_loading(runtime_dir: &Path) -> Result<Pdfium, String> {
    if let Some(path) = resolve_pdfium_library_path(runtime_dir) {
        if let Ok(bindings) = Pdfium::bind_to_library(&path) {
            return Ok(Pdfium::new(bindings));
        }
    }
    if let Ok(bindings) = Pdfium::bind_to_system_library() {
        return Ok(Pdfium::new(bindings));
    }
    Err("Could not load Pdfium. Install runtime or provide pdfium library.".to_owned())
}

fn load_document_payload(
    path: &Path,
    runtime_dir: &Path,
    mut on_status: impl FnMut(String),
) -> Result<LoadedDocumentKind, String> {
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    if extension == "pdf" {
        on_status("0% Opening PDF".to_owned());
        let pdfium = bind_pdfium_for_loading(runtime_dir)?;
        let document = pdfium
            .load_pdf_from_file(path, None)
            .map_err(|err| format!("Failed to open {}: {err}", path.to_string_lossy()))?;
        let pages = document.pages();
        let page_count = pages.len() as usize;
        let mut loaded_pages = Vec::with_capacity(page_count);

        for index in 0..page_count {
            let page = pages
                .get(index as u16)
                .map_err(|err| format!("Failed to read page {}: {err}", index + 1))?;

            let text = page
                .text()
                .map(|text_page| text_page.all())
                .unwrap_or_default();
            let render_config = PdfRenderConfig::new()
                .set_target_width(1500)
                .set_maximum_height(2200);
            let bitmap = page
                .render_with_config(&render_config)
                .map_err(|err| format!("Failed to render page {}: {err}", index + 1))?;
            let rgba = bitmap.as_image().to_rgba8();
            let width = rgba.width() as usize;
            let height = rgba.height() as usize;

            loaded_pages.push(LoadedPdfPagePayload {
                raster: LoadedRaster {
                    width,
                    height,
                    rgba: rgba.into_raw(),
                },
                text,
            });

            let percent = ((index + 1) as f32 / page_count.max(1) as f32) * 100.0;
            on_status(format!(
                "{percent:.1}% Rendering PDF pages ({}/{})",
                index + 1,
                page_count
            ));
        }

        let name = file_name_or_path(path);
        return Ok(LoadedDocumentKind::Pdf {
            pages: loaded_pages,
            markdown: build_unconverted_markdown_placeholder(page_count, &name),
        });
    }

    if matches!(
        extension.as_str(),
        "png" | "jpg" | "jpeg" | "bmp" | "gif" | "webp" | "tif" | "tiff"
    ) {
        on_status("35% Decoding image".to_owned());
        let image = image::open(path)
            .map_err(|err| format!("Failed to load image {}: {err}", path.to_string_lossy()))?;
        let rgba = image.to_rgba8();
        let width = rgba.width() as usize;
        let height = rgba.height() as usize;
        let name = file_name_or_path(path);
        on_status("100% Image ready".to_owned());
        return Ok(LoadedDocumentKind::Image {
            raster: LoadedRaster {
                width,
                height,
                rgba: rgba.into_raw(),
            },
            markdown: build_unconverted_markdown_placeholder(1, &name),
        });
    }

    Err(format!(
        "Unsupported file type for {}",
        path.to_string_lossy()
    ))
}

fn load_rgba_texture(
    ctx: &egui::Context,
    width: usize,
    height: usize,
    rgba: &[u8],
    texture_name: String,
) -> egui::TextureHandle {
    let color_image = ColorImage::from_rgba_unmultiplied([width, height], rgba);
    ctx.load_texture(texture_name, color_image, egui::TextureOptions::LINEAR)
}

fn sanitize_outer_markdown_fence(markdown: &str) -> String {
    let normalized = markdown.replace("\r\n", "\n");
    let lines: Vec<&str> = normalized.lines().collect();
    if lines.is_empty() {
        return normalized;
    }

    let Some(first_non_empty) = lines.iter().position(|line| !line.trim().is_empty()) else {
        return normalized;
    };
    let Some(last_non_empty) = lines.iter().rposition(|line| !line.trim().is_empty()) else {
        return normalized;
    };
    if last_non_empty <= first_non_empty {
        return normalized;
    }

    let opener = lines[first_non_empty].trim();
    if !opener.starts_with("```") {
        return normalized;
    }
    let lang = opener.trim_start_matches("```").trim().to_ascii_lowercase();
    if !(lang.is_empty() || lang == "markdown" || lang == "md") {
        return normalized;
    }
    if lines[last_non_empty].trim() != "```" {
        return normalized;
    }

    let mut out = String::new();
    for (index, line) in lines.iter().enumerate() {
        if index == first_non_empty || index == last_non_empty {
            continue;
        }
        out.push_str(line);
        if index + 1 < lines.len() {
            out.push('\n');
        }
    }
    out.trim_matches('\n').to_owned()
}

fn split_table_cells(line: &str) -> Vec<String> {
    let normalized = normalized_table_pipe_line(line);
    let trimmed = normalized.trim();
    let without_leading = trimmed.strip_prefix('|').unwrap_or(trimmed);
    let core = without_leading.strip_suffix('|').unwrap_or(without_leading);

    let mut cells = Vec::new();
    let mut current = String::new();
    let mut escaped = false;
    for ch in core.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '|' {
            cells.push(current.trim().to_owned());
            current.clear();
            continue;
        }
        current.push(ch);
    }
    if escaped {
        current.push('\\');
    }
    cells.push(current.trim().to_owned());
    cells
}

fn is_markdown_table_separator(line: &str) -> bool {
    let cells = split_table_cells(line);
    !cells.is_empty()
        && cells.iter().all(|cell| {
            let c = cell.trim();
            !c.is_empty() && c.chars().all(|ch| matches!(ch, '-' | ':' | ' '))
        })
}

fn normalized_table_pipe_line(line: &str) -> String {
    line.chars()
        .map(|ch| match ch {
            '\u{2502}' // ¦
            | '\u{2503}' // ?
            | '\u{2551}' // ¦
            | '\u{00A6}' // ¦
            | '\u{FF5C}' // |
            | '\u{FE31}' // ?
            | '\u{23D0}' // ?
            | '\u{2758}' // |
            | '\u{2759}' // ?
            | '\u{275A}' => '|', // ?
            _ => ch,
        })
        .collect()
}
fn markdown_table_column_alignment(cell: &str) -> Align {
    let trimmed = cell.trim();
    let left_aligned = trimmed.starts_with(':');
    let right_aligned = trimmed.ends_with(':');
    match (left_aligned, right_aligned) {
        (true, true) => Align::Center,
        (false, true) => Align::Max,
        _ => Align::Min,
    }
}

fn parse_pipe_table_rows(lines: &[&str]) -> Option<(Vec<Vec<String>>, Vec<Align>)> {
    if lines.len() < 2 {
        return None;
    }

    let normalized_lines: Vec<String> = lines
        .iter()
        .map(|line| normalized_table_pipe_line(line))
        .collect();
    let separator_index = normalized_lines
        .iter()
        .position(|line| is_markdown_table_separator(line))?;
    let separator_cells = split_table_cells(&normalized_lines[separator_index]);

    let mut rows = Vec::new();
    for line in &normalized_lines {
        if is_markdown_table_separator(line) {
            continue;
        }
        let mut cells = split_table_cells(line);
        while cells.last().is_some_and(|cell| cell.is_empty()) {
            cells.pop();
        }
        if cells.is_empty() {
            continue;
        }
        rows.push(cells);
    }

    if rows.len() < 2 {
        return None;
    }

    let max_cols = rows
        .iter()
        .map(std::vec::Vec::len)
        .max()
        .unwrap_or(0)
        .max(1);
    for row in &mut rows {
        row.resize(max_cols, String::new());
    }

    let mut alignments = separator_cells
        .iter()
        .map(|cell| markdown_table_column_alignment(cell))
        .collect::<Vec<_>>();
    alignments.resize(max_cols, Align::Min);
    alignments.truncate(max_cols);

    Some((rows, alignments))
}

fn push_markdown_render_text_block(blocks: &mut Vec<MarkdownRenderBlock>, text_block: &mut String) {
    if text_block.trim().is_empty() {
        text_block.clear();
        return;
    }

    let cleaned = text_block.trim_matches('\n').to_owned();
    if !cleaned.is_empty() {
        blocks.push(MarkdownRenderBlock::Markdown(cleaned));
    }
    text_block.clear();
}

fn split_markdown_render_blocks(markdown: &str) -> Vec<MarkdownRenderBlock> {
    let lines: Vec<&str> = markdown.lines().collect();
    if lines.is_empty() {
        return vec![MarkdownRenderBlock::Markdown(String::new())];
    }

    let mut blocks = Vec::new();
    let mut text_block = String::new();
    let mut i = 0usize;
    let mut in_code_block = false;

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            in_code_block = !in_code_block;
            text_block.push_str(line);
            if i + 1 < lines.len() {
                text_block.push('\n');
            }
            i += 1;
            continue;
        }

        if !in_code_block && line.contains('|') {
            let start = i;
            while i < lines.len() {
                let candidate = lines[i];
                if candidate.trim_start().starts_with("```") || !candidate.contains('|') {
                    break;
                }
                i += 1;
            }

            let table_lines = &lines[start..i];
            if let Some((rows, alignments)) = parse_pipe_table_rows(table_lines) {
                push_markdown_render_text_block(&mut blocks, &mut text_block);
                blocks.push(MarkdownRenderBlock::Table { rows, alignments });
            } else {
                for (row_index, row) in table_lines.iter().enumerate() {
                    text_block.push_str(row);
                    if start + row_index + 1 < lines.len() {
                        text_block.push('\n');
                    }
                }
            }
            continue;
        }

        text_block.push_str(line);
        if i + 1 < lines.len() {
            text_block.push('\n');
        }
        i += 1;
    }

    push_markdown_render_text_block(&mut blocks, &mut text_block);
    if blocks.is_empty() {
        blocks.push(MarkdownRenderBlock::Markdown(markdown.to_owned()));
    }
    blocks
}

fn estimate_table_column_widths(ui: &egui::Ui, rows: &[Vec<String>], font_size: f32) -> Vec<f32> {
    let col_count = rows
        .iter()
        .map(std::vec::Vec::len)
        .max()
        .unwrap_or(0)
        .max(1);
    let mut widths = vec![100.0f32; col_count];
    let body_font = FontId::proportional(font_size.max(8.0));
    let header_font = FontId::proportional((font_size + 0.6).max(8.6));
    let text_color = ui.visuals().text_color();

    for (row_idx, row) in rows.iter().enumerate() {
        for (col_idx, cell) in row.iter().enumerate() {
            let font = if row_idx == 0 {
                header_font.clone()
            } else {
                body_font.clone()
            };
            let galley = ui
                .painter()
                .layout_no_wrap(cell.to_owned(), font, text_color);
            let estimated = galley.size().x + 28.0;
            widths[col_idx] = widths[col_idx].max(estimated);
        }
    }

    for width in &mut widths {
        *width = width.clamp(120.0, 8000.0);
    }

    widths
}

fn render_markdown_table_grid(
    ui: &mut egui::Ui,
    rows: &[Vec<String>],
    alignments: &[Align],
    markdown_font_size: f32,
    document_id: usize,
    page_index: usize,
    block_index: usize,
) {
    if rows.is_empty() {
        return;
    }

    let col_widths = estimate_table_column_widths(ui, rows, markdown_font_size);
    let col_count = col_widths.len();
    let total_width: f32 = col_widths.iter().sum::<f32>() + (col_count as f32 * 2.0) + 12.0;
    let viewport_width = ui.available_width().max(1.0);

    ScrollArea::horizontal()
        .id_salt(("md_page_table_scroll", document_id, page_index, block_index))
        .max_width(viewport_width)
        .min_scrolled_width(viewport_width)
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.set_min_width(total_width.max(viewport_width));
            ui.set_max_width(total_width.max(viewport_width));
            egui::Grid::new(("md_page_table_grid", document_id, page_index, block_index))
                .spacing(egui::vec2(0.0, 0.0))
                .show(ui, |ui| {
                    for (row_idx, row) in rows.iter().enumerate() {
                        let row_height = (markdown_font_size + 14.0).max(24.0);
                        for (col_idx, width) in col_widths.iter().enumerate() {
                            let cell = row.get(col_idx).map_or("", std::string::String::as_str);
                            let alignment = alignments.get(col_idx).copied().unwrap_or(Align::Min);
                            let background = if row_idx == 0 {
                                Color32::from_rgb(241, 244, 248)
                            } else {
                                Color32::from_rgb(253, 253, 254)
                            };
                            let border = Color32::from_rgb(191, 197, 207);
                            let font = if row_idx == 0 {
                                FontId::proportional((markdown_font_size + 0.6).max(8.6))
                            } else {
                                FontId::proportional(markdown_font_size.max(8.0))
                            };
                            let text_color = ui.visuals().text_color();
                            let galley = ui.painter().layout_no_wrap(
                                cell.to_owned(),
                                font.clone(),
                                text_color,
                            );
                            let (rect, _) = ui.allocate_exact_size(
                                egui::vec2(*width, row_height),
                                egui::Sense::hover(),
                            );
                            ui.painter().rect_filled(rect, 0.0, background);
                            ui.painter().rect_stroke(
                                rect,
                                0.0,
                                Stroke::new(1.0, border),
                                egui::StrokeKind::Inside,
                            );
                            let text_rect = rect.shrink2(egui::vec2(8.0, 6.0));
                            let text_x = match alignment {
                                Align::Min => text_rect.left(),
                                Align::Center => (text_rect.center().x - galley.size().x * 0.5)
                                    .max(text_rect.left()),
                                Align::Max => {
                                    (text_rect.right() - galley.size().x).max(text_rect.left())
                                }
                            };
                            let text_y = text_rect.center().y - galley.size().y * 0.5;
                            ui.painter().with_clip_rect(text_rect).galley(
                                egui::pos2(text_x, text_y),
                                galley,
                                text_color,
                            );
                        }
                        ui.end_row();
                    }
                });
        });
}

fn parse_page_marker_fragment(fragment: &str) -> Option<usize> {
    let candidate = fragment.trim_start_matches([':', '#', '-']).trim();
    if candidate.is_empty() {
        return None;
    }

    let mut digits = String::new();
    let mut started = false;
    for ch in candidate.chars() {
        if ch.is_ascii_digit() {
            digits.push(ch);
            started = true;
            continue;
        }
        if !started {
            if ch.is_whitespace() {
                continue;
            }
            return None;
        }
        break;
    }

    if digits.is_empty() {
        None
    } else {
        digits.parse::<usize>().ok()
    }
}

fn parse_page_marker(line: &str) -> Option<(usize, usize, usize)> {
    for (prefix, suffix) in [("<--page", "-->"), ("<-page", "->")] {
        if let Some(start) = line.find(prefix) {
            let after_prefix = start + prefix.len();
            let rest = &line[after_prefix..];
            if let Some(rel_suffix) = rest.find(suffix) {
                let marker_end = after_prefix + rel_suffix + suffix.len();
                let inner = &line[after_prefix..after_prefix + rel_suffix];
                if let Some(page) = parse_page_marker_fragment(inner) {
                    return Some((page, start, marker_end));
                }
            }
        }
    }
    None
}

fn markdown_char_index_for_page(markdown: &str, page_index: usize) -> usize {
    if page_index == 0 {
        return 0;
    }

    let target_page_number = page_index + 1;
    let mut byte_offset = 0usize;

    for segment in markdown.split_inclusive('\n') {
        let line = segment.strip_suffix('\n').unwrap_or(segment);
        if let Some((marker_page, _marker_start, marker_end)) = parse_page_marker(line)
            && marker_page == target_page_number
        {
            let mut target_byte = byte_offset + marker_end;
            let bytes = markdown.as_bytes();
            if target_byte < bytes.len() && bytes[target_byte] == b'\r' {
                target_byte += 1;
            }
            if target_byte < bytes.len() && bytes[target_byte] == b'\n' {
                target_byte += 1;
            }
            while target_byte < bytes.len() && matches!(bytes[target_byte], b' ' | b'\t') {
                target_byte += 1;
            }
            let safe = target_byte.min(markdown.len());
            return markdown[..safe].chars().count();
        }
        byte_offset += segment.len();
    }

    0
}

fn split_markdown_by_page_markers(markdown: &str, page_count: usize) -> Vec<String> {
    let count = page_count.max(1);
    let mut pages = vec![String::new(); count];
    let mut current_page = 0usize;

    for line in markdown.lines() {
        if let Some((marker_page, marker_start, marker_end)) = parse_page_marker(line) {
            let prefix_text = line[..marker_start].trim_end();
            if !prefix_text.is_empty() {
                pages[current_page].push_str(prefix_text);
                pages[current_page].push('\n');
            }

            current_page = marker_page.saturating_sub(1).min(count - 1);

            let suffix_text = line[marker_end..].trim_start();
            if !suffix_text.is_empty() {
                pages[current_page].push_str(suffix_text);
                pages[current_page].push('\n');
            }
            continue;
        }

        pages[current_page].push_str(line);
        pages[current_page].push('\n');
    }

    pages
}

fn build_unconverted_markdown_placeholder(page_count: usize, source_name: &str) -> String {
    let mut markdown = String::new();
    let count = page_count.max(1);
    let message = format!(
        "_Not converted yet for `{}`. Click `Convert Selected` to generate markdown in the source folder._",
        source_name
    );

    for index in 0..count {
        markdown.push_str(&format!("<-page{}->\n\n", index + 1));
        if index == 0 {
            markdown.push_str(&message);
            markdown.push_str("\n\n");
        } else {
            markdown.push_str("_Pending conversion output for this page._\n\n");
        }
    }

    markdown
}

fn count_occurrences_case_insensitive(haystack: &str, needle_lower: &str) -> usize {
    if needle_lower.is_empty() {
        return 0;
    }

    haystack
        .to_ascii_lowercase()
        .match_indices(needle_lower)
        .count()
}

fn excerpt_from_text(text: &str, needle_lower: &str) -> String {
    for line in text.lines() {
        if line.to_ascii_lowercase().contains(needle_lower) {
            return truncate_for_excerpt(line.trim(), 140);
        }
    }

    truncate_for_excerpt(text.trim(), 140)
}

fn truncate_for_excerpt(value: &str, max_chars: usize) -> String {
    let mut output = String::new();
    for (index, ch) in value.chars().enumerate() {
        if index >= max_chars {
            output.push_str("...");
            return output;
        }
        output.push(ch);
    }
    output
}

fn file_name_or_path(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| path.to_string_lossy().to_string())
}

fn parse_gpu_index_setting(value: &str) -> Option<i32> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("none") {
        return None;
    }
    trimmed.parse::<i32>().ok().map(|gpu| gpu.max(0))
}

fn configured_path_exists(path_text: &str) -> bool {
    let trimmed = path_text.trim();
    if trimmed.is_empty() {
        return false;
    }
    Path::new(trimmed).is_file()
}
