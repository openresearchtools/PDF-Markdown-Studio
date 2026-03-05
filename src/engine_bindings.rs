use std::ffi::{CStr, CString};
use std::fs;
use std::os::raw::c_char;
use std::path::{Path, PathBuf};
use std::ptr;

use libloading::Library;

#[derive(Debug, Clone)]
pub struct EngineDevice {
    pub index: i32,
    pub backend: String,
    pub name: String,
    pub description: String,
    pub memory_free: u64,
    pub memory_total: u64,
}

#[derive(Debug, Clone)]
pub struct PdfVlmRequest {
    pub input_path: PathBuf,
    pub is_image: bool,
    pub model_path: PathBuf,
    pub mmproj_path: PathBuf,
    pub output_md_path: PathBuf,
    pub pdfium_lib_path: PathBuf,
    pub prompt: String,
    pub n_predict: i32,
    pub n_ctx: i32,
    pub n_batch: i32,
    pub n_ubatch: i32,
    pub n_parallel: i32,
    pub n_threads: i32,
    pub n_threads_batch: i32,
    pub gpu: Option<i32>,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct llama_server_bridge_device_info {
    index: i32,
    r#type: i32,
    memory_free: u64,
    memory_total: u64,
    backend: *mut c_char,
    name: *mut c_char,
    description: *mut c_char,
}

#[repr(C)]
struct llama_server_bridge {
    _private: [u8; 0],
}

#[repr(C)]
#[derive(Copy, Clone)]
struct llama_server_bridge_params {
    model_path: *const c_char,
    mmproj_path: *const c_char,
    n_ctx: i32,
    n_batch: i32,
    n_ubatch: i32,
    n_parallel: i32,
    n_threads: i32,
    n_threads_batch: i32,
    n_gpu_layers: i32,
    main_gpu: i32,
    gpu: i32,
    no_kv_offload: i32,
    mmproj_use_gpu: i32,
    cache_ram_mib: i32,
    seed: i32,
    ctx_shift: i32,
    kv_unified: i32,
    devices: *const c_char,
    tensor_split: *const c_char,
    split_mode: i32,
    embedding: i32,
    reranking: i32,
    pooling_type: i32,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct llama_server_bridge_vlm_request {
    prompt: *const c_char,
    image_bytes: *const u8,
    image_bytes_len: usize,
    n_predict: i32,
    id_slot: i32,
    temperature: f32,
    top_p: f32,
    top_k: i32,
    min_p: f32,
    seed: i32,
    repeat_last_n: i32,
    repeat_penalty: f32,
    presence_penalty: f32,
    frequency_penalty: f32,
    dry_multiplier: f32,
    dry_allowed_length: i32,
    dry_penalty_last_n: i32,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct llama_server_bridge_vlm_result {
    ok: i32,
    truncated: i32,
    stop: i32,
    n_decoded: i32,
    n_prompt_tokens: i32,
    n_tokens_cached: i32,
    eos_reached: i32,
    prompt_ms: f64,
    predicted_ms: f64,
    text: *mut c_char,
    error_json: *mut c_char,
}

type FnRunFromArgv = unsafe extern "C" fn(i32, *const *const c_char, *mut *mut c_char) -> i32;
type FnFreeCString = unsafe extern "C" fn(*mut c_char);
type FnListDevices =
    unsafe extern "C" fn(*mut *mut llama_server_bridge_device_info, *mut usize) -> i32;
type FnFreeDevices = unsafe extern "C" fn(*mut llama_server_bridge_device_info, usize);
type FnBridgeDefaultParams = unsafe extern "C" fn() -> llama_server_bridge_params;
type FnBridgeDefaultVlmRequest = unsafe extern "C" fn() -> llama_server_bridge_vlm_request;
type FnBridgeEmptyVlmResult = unsafe extern "C" fn() -> llama_server_bridge_vlm_result;
type FnBridgeCreate =
    unsafe extern "C" fn(*const llama_server_bridge_params) -> *mut llama_server_bridge;
type FnBridgeDestroy = unsafe extern "C" fn(*mut llama_server_bridge);
type FnBridgeVlmComplete = unsafe extern "C" fn(
    *mut llama_server_bridge,
    *const llama_server_bridge_vlm_request,
    *mut llama_server_bridge_vlm_result,
) -> i32;
type FnBridgeResultFree = unsafe extern "C" fn(*mut llama_server_bridge_vlm_result);
type FnBridgeLastError = unsafe extern "C" fn(*const llama_server_bridge) -> *const c_char;

pub fn runtime_pdfium_library_path(runtime_dir: &Path) -> PathBuf {
    if cfg!(target_os = "windows") {
        runtime_dir.join("vendor").join("pdfium").join("pdfium.dll")
    } else if cfg!(target_os = "macos") {
        runtime_dir
            .join("vendor")
            .join("pdfium")
            .join("libpdfium.dylib")
    } else {
        runtime_dir
            .join("vendor")
            .join("pdfium")
            .join("libpdfium.so")
    }
}

pub fn run_pdf_fast(runtime_dir: &Path, input_pdf: &Path, output_md: &Path) -> Result<(), String> {
    let lib_name = if cfg!(target_os = "windows") {
        "pdf.dll"
    } else if cfg!(target_os = "macos") {
        "libpdf.dylib"
    } else {
        "libpdf.so"
    };
    let library_path = runtime_dir.join(lib_name);
    let pdfium_lib = runtime_pdfium_library_path(runtime_dir);
    let args = vec![
        "pdf".to_owned(),
        "--pdfium-lib".to_owned(),
        pdfium_lib.display().to_string(),
        "extract".to_owned(),
        "--input".to_owned(),
        input_pdf.display().to_string(),
        "--output".to_owned(),
        output_md.display().to_string(),
        "--overwrite".to_owned(),
    ];

    run_argv_library(
        runtime_dir,
        &library_path,
        b"pdf_run_from_argv\0",
        b"pdf_free_c_string\0",
        &args,
    )
}

pub fn run_pdf_vlm(runtime_dir: &Path, request: &PdfVlmRequest) -> Result<(), String> {
    if request.is_image {
        return Err(
            "run_pdf_vlm() requires a PDF input; image inputs must use run_image_vlm().".to_owned(),
        );
    }

    let lib_name = if cfg!(target_os = "windows") {
        "pdfvlm.dll"
    } else if cfg!(target_os = "macos") {
        "libpdfvlm.dylib"
    } else {
        "libpdfvlm.so"
    };
    let library_path = runtime_dir.join(lib_name);

    let selected_gpu = request.gpu.map(|gpu| gpu.max(0));
    let format_error = |err: String| {
        if selected_gpu.is_some() && err.contains("Unknown argument: --gpu") {
            return format!(
                "{err} (effective gpu={}) (expected --gpu <index> support; verify runtime is loading the correct/newer pdfvlm library at '{}')",
                describe_gpu(request.gpu),
                library_path.display()
            );
        }
        if selected_gpu.is_none() && err.contains("llama_server_bridge_create() failed") {
            return format!(
                "{err} (effective gpu={}) (CPU mode init failed; reduce n_ctx/n_batch/n_ubatch/n_parallel or allocate more RAM)",
                describe_gpu(request.gpu)
            );
        }
        format!("{err} (effective gpu={})", describe_gpu(request.gpu))
    };

    let run_with = |n_ctx: i32,
                    n_batch: i32,
                    n_ubatch: i32,
                    n_parallel: i32,
                    n_threads: i32,
                    n_threads_batch: i32|
     -> Result<(), String> {
        let mut args = vec!["pdf_to_markdown".to_owned()];
        args.push("--pdf".to_owned());
        args.push(request.input_path.display().to_string());
        args.push("--pdfium-lib".to_owned());
        args.push(request.pdfium_lib_path.display().to_string());
        args.push("--model".to_owned());
        args.push(request.model_path.display().to_string());
        args.push("--mmproj".to_owned());
        args.push(request.mmproj_path.display().to_string());
        args.push("--out-md".to_owned());
        args.push(request.output_md_path.display().to_string());

        if !request.prompt.trim().is_empty() {
            args.push("--prompt".to_owned());
            args.push(request.prompt.clone());
        }

        args.push("--n-predict".to_owned());
        args.push(request.n_predict.to_string());
        args.push("--n-ctx".to_owned());
        args.push(n_ctx.max(1).to_string());
        // pdf_to_markdown does not expose a separate u-batch flag; honor UI u-batch
        // by constraining effective batch-size to min(batch, u-batch).
        let effective_batch = n_batch.max(1).min(n_ubatch.max(1));
        args.push("--batch-size".to_owned());
        args.push(effective_batch.to_string());

        let effective_threads = n_threads.max(1);
        let effective_threads_batch = n_threads_batch.max(1);
        args.push("--threads".to_owned());
        args.push(effective_threads.to_string());
        args.push("--threads-batch".to_owned());
        args.push(effective_threads_batch.to_string());

        args.push("--parallel".to_owned());
        args.push(n_parallel.max(1).to_string());

        if let Some(gpu) = selected_gpu {
            // Preferred selector form for pdfvlm: two-token flag + value.
            args.push("--gpu".to_owned());
            args.push(gpu.to_string());
            args.push("--mmproj-use-gpu".to_owned());
            args.push("1".to_owned());
            args.push("--n-gpu-layers".to_owned());
            args.push("-1".to_owned());
            args.push("--split-mode".to_owned());
            args.push("none".to_owned());
        } else {
            // Force CPU mode across all platforms (including macOS defaults).
            args.push("--devices".to_owned());
            args.push("none".to_owned());
            args.push("--mmproj-use-gpu".to_owned());
            args.push("0".to_owned());
            args.push("--n-gpu-layers".to_owned());
            args.push("0".to_owned());
            args.push("--split-mode".to_owned());
            args.push("none".to_owned());
        }

        run_argv_library(
            runtime_dir,
            &library_path,
            b"pdfvlm_run_from_argv\0",
            b"pdfvlm_free_c_string\0",
            &args,
        )
        .map_err(format_error)
    };

    let effective_threads = resolve_effective_threads(request.n_threads);
    let effective_threads_batch = if request.n_threads_batch > 0 {
        request.n_threads_batch.max(1)
    } else {
        effective_threads
    };

    run_with(
        request.n_ctx,
        request.n_batch,
        request.n_ubatch,
        request.n_parallel,
        effective_threads,
        effective_threads_batch,
    )
}

pub fn run_image_vlm(runtime_dir: &Path, request: &PdfVlmRequest) -> Result<(), String> {
    if !request.is_image {
        return Err("run_image_vlm() requires an image input.".to_owned());
    }

    configure_runtime_loader_paths(runtime_dir);

    let library_path = if cfg!(target_os = "windows") {
        runtime_dir.join("llama-server-bridge.dll")
    } else if cfg!(target_os = "macos") {
        runtime_dir.join("libllama-server-bridge.dylib")
    } else {
        runtime_dir.join("libllama-server-bridge.so")
    };
    if !library_path.exists() {
        return Err(format!(
            "missing bridge library '{}'",
            library_path.display()
        ));
    }

    let image_bytes = fs::read(&request.input_path).map_err(|err| {
        format!(
            "failed to read image '{}': {err}",
            request.input_path.display()
        )
    })?;
    if image_bytes.is_empty() {
        return Err(format!("image '{}' is empty", request.input_path.display()));
    }

    let model_c = CString::new(request.model_path.display().to_string())
        .map_err(|_| "model path contains NUL byte".to_owned())?;
    let mmproj_c = CString::new(request.mmproj_path.display().to_string())
        .map_err(|_| "mmproj path contains NUL byte".to_owned())?;
    let prompt_text = if request.prompt.trim().is_empty() {
        "Describe this image in markdown.".to_owned()
    } else {
        request.prompt.clone()
    };
    let prompt_c = CString::new(prompt_text).map_err(|_| "prompt contains NUL byte".to_owned())?;
    let cpu_only_devices_c = if request.gpu.is_none() {
        Some(CString::new("none").map_err(|_| "devices contains NUL byte".to_owned())?)
    } else {
        None
    };
    let library = unsafe { Library::new(&library_path) }
        .map_err(|err| format!("failed to load '{}': {err}", library_path.display()))?;
    let default_params = unsafe {
        *library
            .get::<FnBridgeDefaultParams>(b"llama_server_bridge_default_params\0")
            .map_err(|err| format!("missing symbol llama_server_bridge_default_params: {err}"))?
    };
    let default_vlm_request = unsafe {
        *library
            .get::<FnBridgeDefaultVlmRequest>(b"llama_server_bridge_default_vlm_request\0")
            .map_err(|err| {
                format!("missing symbol llama_server_bridge_default_vlm_request: {err}")
            })?
    };
    let empty_vlm_result = unsafe {
        *library
            .get::<FnBridgeEmptyVlmResult>(b"llama_server_bridge_empty_vlm_result\0")
            .map_err(|err| format!("missing symbol llama_server_bridge_empty_vlm_result: {err}"))?
    };
    let bridge_create = unsafe {
        *library
            .get::<FnBridgeCreate>(b"llama_server_bridge_create\0")
            .map_err(|err| format!("missing symbol llama_server_bridge_create: {err}"))?
    };
    let bridge_destroy = unsafe {
        *library
            .get::<FnBridgeDestroy>(b"llama_server_bridge_destroy\0")
            .map_err(|err| format!("missing symbol llama_server_bridge_destroy: {err}"))?
    };
    let bridge_vlm_complete = unsafe {
        *library
            .get::<FnBridgeVlmComplete>(b"llama_server_bridge_vlm_complete\0")
            .map_err(|err| format!("missing symbol llama_server_bridge_vlm_complete: {err}"))?
    };
    let bridge_result_free = unsafe {
        *library
            .get::<FnBridgeResultFree>(b"llama_server_bridge_result_free\0")
            .map_err(|err| format!("missing symbol llama_server_bridge_result_free: {err}"))?
    };
    let bridge_last_error = unsafe {
        *library
            .get::<FnBridgeLastError>(b"llama_server_bridge_last_error\0")
            .map_err(|err| format!("missing symbol llama_server_bridge_last_error: {err}"))?
    };

    let markdown = with_runtime_cwd(runtime_dir, || {
        let effective_threads = resolve_effective_threads(request.n_threads);
        let effective_threads_batch = if request.n_threads_batch > 0 {
            request.n_threads_batch.max(1)
        } else {
            effective_threads
        };
        let build_params = |n_ctx: i32, n_batch: i32, n_ubatch: i32, n_parallel: i32| {
            let mut params = unsafe { default_params() };
            params.model_path = model_c.as_ptr();
            params.mmproj_path = mmproj_c.as_ptr();
            params.n_ctx = n_ctx.max(1);
            params.n_batch = n_batch.max(1);
            params.n_ubatch = n_ubatch.max(1);
            params.n_parallel = n_parallel.max(1);
            params.n_threads = effective_threads;
            params.n_threads_batch = effective_threads_batch;
            params.main_gpu = -1;
            if let Some(gpu) = request.gpu {
                params.gpu = gpu.max(0);
                params.devices = ptr::null();
                params.n_gpu_layers = -1;
                params.mmproj_use_gpu = 1;
                params.split_mode = 0;
            } else {
                params.gpu = -1;
                params.devices = cpu_only_devices_c
                    .as_ref()
                    .map_or(ptr::null(), |value| value.as_ptr());
                params.n_gpu_layers = 0;
                params.mmproj_use_gpu = 0;
                params.split_mode = 0;
            }
            params.no_kv_offload = 0;
            params.kv_unified = 1;
            params.ctx_shift = 1;
            params.tensor_split = ptr::null();
            params.embedding = 0;
            params.reranking = 0;
            params.pooling_type = -1;
            params
        };

        let params = build_params(
            request.n_ctx.max(1),
            request.n_batch.max(1),
            request.n_ubatch.max(1),
            request.n_parallel.max(1),
        );
        let bridge = unsafe { bridge_create(&params) };
        if bridge.is_null() {
            return Err(format!(
                "llama_server_bridge_create() failed (effective gpu={}) (n_ctx={} batch={} ubatch={} parallel={} threads={} threads_batch={})",
                describe_gpu(request.gpu),
                params.n_ctx,
                params.n_batch,
                params.n_ubatch,
                params.n_parallel,
                params.n_threads,
                params.n_threads_batch
            ));
        }

        let mut req_ffi = unsafe { default_vlm_request() };
        req_ffi.prompt = prompt_c.as_ptr();
        req_ffi.image_bytes = image_bytes.as_ptr();
        req_ffi.image_bytes_len = image_bytes.len();
        req_ffi.n_predict = request.n_predict.max(1);
        req_ffi.id_slot = -1;
        req_ffi.temperature = 0.0;
        req_ffi.top_p = 1.0;
        req_ffi.top_k = -1;
        req_ffi.min_p = -1.0;
        req_ffi.seed = -1;

        let mut out = unsafe { empty_vlm_result() };
        let rc = unsafe { bridge_vlm_complete(bridge, &req_ffi, &mut out) };
        let text = cstr_from_mut(out.text);
        let out_err = cstr_from_mut(out.error_json);

        if rc != 0 || out.ok == 0 {
            let bridge_err = cstr_from_const(unsafe { bridge_last_error(bridge) });
            unsafe {
                bridge_result_free(&mut out);
                bridge_destroy(bridge);
            }
            return Err(format!(
                "image VLM failed rc={} ok={} bridge_err='{}' out_err='{}'",
                rc, out.ok, bridge_err, out_err
            ));
        }

        unsafe {
            bridge_result_free(&mut out);
            bridge_destroy(bridge);
        }
        Ok(text)
    })??;

    if let Some(parent) = request.output_md_path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "failed to create output directory '{}': {err}",
                parent.display()
            )
        })?;
    }
    fs::write(&request.output_md_path, markdown).map_err(|err| {
        format!(
            "failed writing image markdown output '{}': {err}",
            request.output_md_path.display()
        )
    })?;

    Ok(())
}

pub fn list_bridge_devices(runtime_dir: &Path) -> Result<Vec<EngineDevice>, String> {
    configure_runtime_loader_paths(runtime_dir);

    let library_path = if cfg!(target_os = "windows") {
        runtime_dir.join("llama-server-bridge.dll")
    } else if cfg!(target_os = "macos") {
        runtime_dir.join("libllama-server-bridge.dylib")
    } else {
        runtime_dir.join("libllama-server-bridge.so")
    };
    if !library_path.exists() {
        return Err(format!(
            "missing bridge library '{}'",
            library_path.display()
        ));
    }

    let library = unsafe { Library::new(&library_path) }
        .map_err(|err| format!("failed to load '{}': {err}", library_path.display()))?;

    let list_devices = unsafe {
        *library
            .get::<FnListDevices>(b"llama_server_bridge_list_devices\0")
            .map_err(|err| format!("missing symbol llama_server_bridge_list_devices: {err}"))?
    };
    let free_devices = unsafe {
        *library
            .get::<FnFreeDevices>(b"llama_server_bridge_free_devices\0")
            .map_err(|err| format!("missing symbol llama_server_bridge_free_devices: {err}"))?
    };

    let devices = with_runtime_cwd(runtime_dir, || {
        let mut ptr_devices = ptr::null_mut();
        let mut count = 0usize;
        let rc = unsafe { list_devices(&mut ptr_devices, &mut count) };
        if rc != 0 {
            return Err(format!("llama_server_bridge_list_devices failed (rc={rc})"));
        }

        let mut devices = Vec::with_capacity(count);
        for idx in 0..count {
            let info = unsafe { &*ptr_devices.add(idx) };
            devices.push(EngineDevice {
                index: info.index,
                backend: cstr_from_mut(info.backend),
                name: cstr_from_mut(info.name),
                description: cstr_from_mut(info.description),
                memory_free: info.memory_free,
                memory_total: info.memory_total,
            });
        }

        unsafe {
            free_devices(ptr_devices, count);
        }

        Ok(devices)
    })??;
    Ok(devices)
}

fn run_argv_library(
    runtime_dir: &Path,
    library_path: &Path,
    run_symbol: &[u8],
    free_symbol: &[u8],
    args: &[String],
) -> Result<(), String> {
    configure_runtime_loader_paths(runtime_dir);

    if !library_path.exists() {
        return Err(format!(
            "runtime library not found '{}'",
            library_path.display()
        ));
    }

    let library = unsafe { Library::new(library_path) }
        .map_err(|err| format!("failed to load '{}': {err}", library_path.display()))?;
    let run_from_argv = unsafe {
        *library
            .get::<FnRunFromArgv>(run_symbol)
            .map_err(|err| format!("missing symbol: {err}"))?
    };
    let free_c_string = unsafe {
        *library
            .get::<FnFreeCString>(free_symbol)
            .map_err(|err| format!("missing symbol: {err}"))?
    };

    let c_args = args
        .iter()
        .map(|arg| CString::new(arg.as_str()).map_err(|_| "argument contains NUL byte".to_owned()))
        .collect::<Result<Vec<_>, _>>()?;
    let argv = c_args.iter().map(|arg| arg.as_ptr()).collect::<Vec<_>>();

    let mut out_error: *mut c_char = ptr::null_mut();
    let rc = with_runtime_cwd(runtime_dir, || unsafe {
        run_from_argv(argv.len() as i32, argv.as_ptr(), &mut out_error)
    })?;

    if rc != 0 {
        let error_message = if out_error.is_null() {
            format!(
                "runtime call failed rc={rc} using '{}'",
                library_path.display()
            )
        } else {
            let message = cstr_from_const(out_error as *const c_char);
            unsafe {
                free_c_string(out_error);
            }
            message
        };
        return Err(error_message);
    }

    if !out_error.is_null() {
        unsafe {
            free_c_string(out_error);
        }
    }

    Ok(())
}

fn cstr_from_const(ptr: *const c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned()
}

fn cstr_from_mut(ptr: *mut c_char) -> String {
    cstr_from_const(ptr as *const c_char)
}

fn describe_gpu(gpu: Option<i32>) -> String {
    gpu.map(|value| value.max(0).to_string())
        .unwrap_or_else(|| "cpu".to_owned())
}

fn resolve_effective_threads(requested_threads: i32) -> i32 {
    if requested_threads > 0 {
        return requested_threads.max(1);
    }

    #[cfg(target_os = "linux")]
    {
        let available = std::thread::available_parallelism()
            .map(|value| value.get())
            .unwrap_or(8);
        return available.saturating_sub(1).max(1).min(8) as i32;
    }

    8
}

#[cfg(windows)]
fn configure_runtime_loader_paths(runtime_dir: &Path) {
    use std::collections::HashSet;
    use std::iter;
    use std::os::windows::ffi::OsStrExt;

    const LOAD_LIBRARY_SEARCH_DEFAULT_DIRS: u32 = 0x00001000;
    const LOAD_LIBRARY_SEARCH_USER_DIRS: u32 = 0x00000400;

    unsafe extern "system" {
        fn SetDefaultDllDirectories(directory_flags: u32) -> i32;
        fn AddDllDirectory(new_directory: *const u16) -> *mut core::ffi::c_void;
        fn SetDllDirectoryW(path_name: *const u16) -> i32;
    }

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str()
            .encode_wide()
            .chain(iter::once(0))
            .collect()
    }

    let mut dirs = vec![runtime_dir.to_path_buf()];
    dirs.push(runtime_dir.join("vendor").join("ffmpeg").join("bin"));
    dirs.push(runtime_dir.join("vendor").join("ffmpeg"));
    dirs.push(runtime_dir.join("vendor").join("pdfium"));
    dirs.push(runtime_dir.join("vendor").join("cuda"));
    if let Ok(cwd) = std::env::current_dir() {
        dirs.push(cwd.join("vendor").join("ffmpeg").join("bin"));
        dirs.push(cwd.join("vendor").join("ffmpeg"));
    }
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            dirs.push(exe_dir.to_path_buf());
        }
    }

    let mut seen = HashSet::new();
    let mut deduped = Vec::new();
    for dir in dirs {
        let abs = dir.canonicalize().unwrap_or(dir);
        if !abs.exists() {
            continue;
        }
        let key = abs.to_string_lossy().to_string();
        if seen.insert(key) {
            deduped.push(abs);
        }
    }

    unsafe {
        let _ = SetDefaultDllDirectories(
            LOAD_LIBRARY_SEARCH_DEFAULT_DIRS | LOAD_LIBRARY_SEARCH_USER_DIRS,
        );
    }
    for dir in &deduped {
        let wide_dir = wide(dir);
        unsafe {
            let cookie = AddDllDirectory(wide_dir.as_ptr());
            if cookie.is_null() {
                let _ = SetDllDirectoryW(wide_dir.as_ptr());
            }
        }
    }
}

#[cfg(not(windows))]
fn configure_runtime_loader_paths(runtime_dir: &Path) {
    use std::collections::HashSet;

    let mut dirs = vec![runtime_dir.to_path_buf()];
    dirs.push(runtime_dir.join("vendor").join("ffmpeg").join("bin"));
    dirs.push(runtime_dir.join("vendor").join("ffmpeg"));
    dirs.push(runtime_dir.join("vendor").join("pdfium"));
    dirs.push(runtime_dir.join("vendor").join("cuda"));
    if let Ok(cwd) = std::env::current_dir() {
        dirs.push(cwd.join("vendor").join("ffmpeg").join("bin"));
        dirs.push(cwd.join("vendor").join("ffmpeg"));
    }
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            dirs.push(exe_dir.to_path_buf());
        }
    }

    let mut seen = HashSet::new();
    let mut merged = Vec::new();
    for dir in dirs {
        let abs = dir.canonicalize().unwrap_or(dir);
        if !abs.exists() {
            continue;
        }
        let key = abs.to_string_lossy().to_string();
        if seen.insert(key) {
            merged.push(abs);
        }
    }

    #[cfg(target_os = "macos")]
    let library_vars: &[&str] = &["DYLD_LIBRARY_PATH", "DYLD_FALLBACK_LIBRARY_PATH"];
    #[cfg(not(target_os = "macos"))]
    let library_vars: &[&str] = &["LD_LIBRARY_PATH"];

    for var_name in library_vars {
        let existing = std::env::var_os(var_name);
        if let Some(value) = existing {
            for path in std::env::split_paths(&value) {
                if path.as_os_str().is_empty() {
                    continue;
                }
                let key = path.to_string_lossy().to_string();
                if seen.insert(key) {
                    merged.push(path);
                }
            }
        }

        if let Ok(joined) = std::env::join_paths(&merged) {
            // SAFETY: we mutate process env before each FFI runtime load call.
            // This app serializes runtime CWD-sensitive operations via a global lock.
            unsafe {
                std::env::set_var(var_name, joined);
            }
        }
    }
}

#[cfg(windows)]
fn with_runtime_cwd<T>(runtime_dir: &Path, f: impl FnOnce() -> T) -> Result<T, String> {
    use std::sync::{Mutex, OnceLock};

    static CWD_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let lock = CWD_LOCK.get_or_init(|| Mutex::new(()));
    let _guard = lock
        .lock()
        .map_err(|_| "failed to lock runtime cwd guard".to_owned())?;

    let previous = std::env::current_dir()
        .map_err(|err| format!("failed to read current working directory: {err}"))?;
    std::env::set_current_dir(runtime_dir).map_err(|err| {
        format!(
            "failed to set working directory to '{}': {err}",
            runtime_dir.display()
        )
    })?;

    struct ResetGuard {
        previous: PathBuf,
    }
    impl Drop for ResetGuard {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.previous);
        }
    }
    let _reset = ResetGuard { previous };

    Ok(f())
}

#[cfg(not(windows))]
fn with_runtime_cwd<T>(runtime_dir: &Path, f: impl FnOnce() -> T) -> Result<T, String> {
    use std::sync::{Mutex, OnceLock};

    static CWD_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let lock = CWD_LOCK.get_or_init(|| Mutex::new(()));
    let _guard = lock
        .lock()
        .map_err(|_| "failed to lock runtime cwd guard".to_owned())?;

    let previous = std::env::current_dir()
        .map_err(|err| format!("failed to read current working directory: {err}"))?;
    std::env::set_current_dir(runtime_dir).map_err(|err| {
        format!(
            "failed to set working directory to '{}': {err}",
            runtime_dir.display()
        )
    })?;

    struct ResetGuard {
        previous: PathBuf,
    }
    impl Drop for ResetGuard {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.previous);
        }
    }
    let _reset = ResetGuard { previous };

    Ok(f())
}
