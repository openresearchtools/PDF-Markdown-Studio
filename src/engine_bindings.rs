#[cfg(not(target_os = "linux"))]
use std::ffi::{CStr, CString};
#[cfg(not(target_os = "linux"))]
use std::fs;
#[cfg(not(target_os = "linux"))]
use std::os::raw::c_char;
use std::path::{Path, PathBuf};
#[cfg(target_os = "linux")]
use std::process::Command;
#[cfg(not(target_os = "linux"))]
use std::ptr;

#[cfg(not(target_os = "linux"))]
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
#[cfg(not(target_os = "linux"))]
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
#[cfg(not(target_os = "linux"))]
struct llama_server_bridge {
    _private: [u8; 0],
}

#[repr(C)]
#[derive(Copy, Clone)]
#[cfg(not(target_os = "linux"))]
struct llama_server_bridge_params {
    model_path: *const c_char,
    mmproj_path: *const c_char,
    cluster_instance_name: *const c_char,
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
    use_mmap: i32,
    use_direct_io: i32,
    use_mlock: i32,
    no_host: i32,
    no_extra_bufts: i32,
    devices: *const c_char,
    tensor_split: *const c_char,
    split_mode: i32,
    embedding: i32,
    reranking: i32,
    pooling_type: i32,
}

#[repr(C)]
#[derive(Copy, Clone)]
#[cfg(not(target_os = "linux"))]
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
    reasoning: *const c_char,
    reasoning_budget: i32,
    reasoning_format: *const c_char,
}

#[repr(C)]
#[derive(Copy, Clone)]
#[cfg(not(target_os = "linux"))]
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

#[cfg(not(target_os = "linux"))]
type FnRunFromArgv = unsafe extern "C" fn(i32, *const *const c_char, *mut *mut c_char) -> i32;
#[cfg(not(target_os = "linux"))]
type FnFreeCString = unsafe extern "C" fn(*mut c_char);
#[cfg(not(target_os = "linux"))]
type FnListDevices =
    unsafe extern "C" fn(*mut *mut llama_server_bridge_device_info, *mut usize) -> i32;
#[cfg(not(target_os = "linux"))]
type FnFreeDevices = unsafe extern "C" fn(*mut llama_server_bridge_device_info, usize);
#[cfg(not(target_os = "linux"))]
type FnBridgeDefaultParams = unsafe extern "C" fn() -> llama_server_bridge_params;
#[cfg(not(target_os = "linux"))]
type FnBridgeDefaultVlmRequest = unsafe extern "C" fn() -> llama_server_bridge_vlm_request;
#[cfg(not(target_os = "linux"))]
type FnBridgeEmptyVlmResult = unsafe extern "C" fn() -> llama_server_bridge_vlm_result;
#[cfg(not(target_os = "linux"))]
type FnBridgeCreate =
    unsafe extern "C" fn(*const llama_server_bridge_params) -> *mut llama_server_bridge;
#[cfg(not(target_os = "linux"))]
type FnBridgeDestroy = unsafe extern "C" fn(*mut llama_server_bridge);
#[cfg(not(target_os = "linux"))]
type FnBridgeVlmComplete = unsafe extern "C" fn(
    *mut llama_server_bridge,
    *const llama_server_bridge_vlm_request,
    *mut llama_server_bridge_vlm_result,
) -> i32;
#[cfg(not(target_os = "linux"))]
type FnBridgeResultFree = unsafe extern "C" fn(*mut llama_server_bridge_vlm_result);
#[cfg(not(target_os = "linux"))]
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
    #[cfg(target_os = "linux")]
    {
        let args = vec![
            "pdf".to_owned(),
            "--pdfium-lib".to_owned(),
            runtime_pdfium_library_path(runtime_dir)
                .display()
                .to_string(),
            "extract".to_owned(),
            "--input".to_owned(),
            input_pdf.display().to_string(),
            "--output".to_owned(),
            output_md.display().to_string(),
            "--overwrite".to_owned(),
        ];
        return run_runtime_cli(runtime_dir, &args).map(|_| ());
    }

    #[cfg(not(target_os = "linux"))]
    {
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
}

pub fn run_pdf_vlm(runtime_dir: &Path, request: &PdfVlmRequest) -> Result<(), String> {
    if request.is_image {
        return Err(
            "run_pdf_vlm() requires a PDF input; image inputs must use run_image_vlm().".to_owned(),
        );
    }

    #[cfg(target_os = "linux")]
    {
        return run_pdf_vlm_cli(runtime_dir, request);
    }

    #[cfg(not(target_os = "linux"))]
    {
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
            args.push("--reasoning".to_owned());
            args.push("off".to_owned());

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
}

pub fn run_image_vlm(runtime_dir: &Path, request: &PdfVlmRequest) -> Result<(), String> {
    if !request.is_image {
        return Err("run_image_vlm() requires an image input.".to_owned());
    }

    #[cfg(target_os = "linux")]
    {
        return run_pdf_vlm_cli(runtime_dir, request);
    }

    #[cfg(not(target_os = "linux"))]
    {
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
        let prompt_c =
            CString::new(prompt_text).map_err(|_| "prompt contains NUL byte".to_owned())?;
        let reasoning_off_c =
            CString::new("off").map_err(|_| "reasoning mode contains NUL byte".to_owned())?;
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
                .map_err(|err| {
                    format!("missing symbol llama_server_bridge_default_params: {err}")
                })?
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
                .map_err(|err| {
                    format!("missing symbol llama_server_bridge_empty_vlm_result: {err}")
                })?
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
                params.use_mmap = 0;
                params.use_direct_io = 0;
                params.use_mlock = 0;
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
            req_ffi.reasoning = reasoning_off_c.as_ptr();
            req_ffi.reasoning_budget = 0;

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
}

pub fn list_bridge_devices(runtime_dir: &Path) -> Result<Vec<EngineDevice>, String> {
    #[cfg(target_os = "linux")]
    {
        let output = run_runtime_cli(runtime_dir, &["list-devices".to_owned()])?;
        return parse_list_devices_output(&output);
    }

    #[cfg(not(target_os = "linux"))]
    {
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
}

#[cfg(target_os = "linux")]
fn run_pdf_vlm_cli(runtime_dir: &Path, request: &PdfVlmRequest) -> Result<(), String> {
    let args = build_pdf_vlm_cli_args(request);

    run_runtime_cli(runtime_dir, &args)
        .map(|_| ())
        .map_err(|err| format!("{err} (effective gpu={})", describe_gpu(request.gpu)))
}

#[cfg(target_os = "linux")]
fn build_pdf_vlm_cli_args(request: &PdfVlmRequest) -> Vec<String> {
    let selected_gpu = request.gpu.map(|gpu| gpu.max(0));
    let mut args = vec!["pdfvlm".to_owned()];
    args.push(if request.is_image {
        "--image".to_owned()
    } else {
        "--pdf".to_owned()
    });
    args.push(request.input_path.display().to_string());
    if !request.is_image {
        args.push("--pdfium-lib".to_owned());
        args.push(request.pdfium_lib_path.display().to_string());
    }
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
    args.push("--reasoning".to_owned());
    args.push("off".to_owned());
    args.push("--n-predict".to_owned());
    args.push(request.n_predict.max(1).to_string());
    args.push("--n-ctx".to_owned());
    args.push(request.n_ctx.max(1).to_string());
    args.push("--batch-size".to_owned());
    args.push(
        request
            .n_batch
            .max(1)
            .min(request.n_ubatch.max(1))
            .to_string(),
    );
    args.push("--threads".to_owned());
    args.push(resolve_effective_threads(request.n_threads).to_string());
    args.push("--threads-batch".to_owned());
    args.push(
        if request.n_threads_batch > 0 {
            request.n_threads_batch.max(1)
        } else {
            resolve_effective_threads(request.n_threads)
        }
        .to_string(),
    );
    args.push("--parallel".to_owned());
    args.push(request.n_parallel.max(1).to_string());

    if let Some(gpu) = selected_gpu {
        args.push("--gpu".to_owned());
        args.push(gpu.to_string());
        args.push("--mmproj-use-gpu".to_owned());
        args.push("1".to_owned());
        args.push("--n-gpu-layers".to_owned());
        args.push("-1".to_owned());
    } else {
        args.push("--devices".to_owned());
        args.push("none".to_owned());
        args.push("--mmproj-use-gpu".to_owned());
        args.push("0".to_owned());
        args.push("--n-gpu-layers".to_owned());
        args.push("0".to_owned());
    }
    args.push("--split-mode".to_owned());
    args.push("none".to_owned());

    args
}

#[cfg(target_os = "linux")]
fn runtime_library_path(runtime_dir: &Path) -> Result<std::ffi::OsString, String> {
    let paths = vec![
        runtime_dir.to_path_buf(),
        runtime_dir.join("vendor").join("cuda"),
        runtime_dir.join("vendor").join("ffmpeg").join("lib"),
        runtime_dir.join("vendor").join("ffmpeg").join("bin"),
        runtime_dir.join("vendor").join("pdfium"),
    ];
    std::env::join_paths(paths)
        .map_err(|err| format!("failed to build runtime library path: {err}"))
}

#[cfg(target_os = "linux")]
fn run_runtime_cli(runtime_dir: &Path, args: &[String]) -> Result<String, String> {
    let cli = runtime_dir.join("example-cli");
    if !cli.is_file() {
        return Err(format!("missing Engine CLI '{}'", cli.display()));
    }

    let output = Command::new(&cli)
        .args(args)
        .current_dir(runtime_dir)
        .env("LD_LIBRARY_PATH", runtime_library_path(runtime_dir)?)
        .output()
        .map_err(|err| format!("failed to run '{}': {err}", cli.display()))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = match (stdout.trim().is_empty(), stderr.trim().is_empty()) {
        (false, false) => format!("{stdout}\n{stderr}"),
        (false, true) => stdout.into_owned(),
        (true, false) => stderr.into_owned(),
        (true, true) => String::new(),
    };

    if !output.status.success() {
        return Err(format!(
            "Engine CLI failed with {} using '{}': {}",
            output.status,
            cli.display(),
            combined.trim()
        ));
    }
    Ok(combined)
}

#[cfg(target_os = "linux")]
fn parse_list_devices_output(output: &str) -> Result<Vec<EngineDevice>, String> {
    fn between<'a>(line: &'a str, start: &str, end: &str) -> Option<&'a str> {
        let value = line.split_once(start)?.1;
        Some(value.split_once(end)?.0.trim())
    }

    let mut devices = Vec::new();
    for line in output.lines().map(str::trim) {
        if !line.starts_with("device index=") {
            continue;
        }
        let index = between(line, "device index=", " backend=")
            .and_then(|value| value.parse::<i32>().ok())
            .ok_or_else(|| format!("invalid Engine device index line: {line}"))?;
        let backend = between(line, " backend=", " name=")
            .ok_or_else(|| format!("missing Engine device backend: {line}"))?
            .to_owned();
        let name = between(line, " name=", " desc=")
            .ok_or_else(|| format!("missing Engine device name: {line}"))?
            .to_owned();
        let description = between(line, " desc=", " type=")
            .ok_or_else(|| format!("missing Engine device description: {line}"))?
            .to_owned();
        let memory_free = between(line, " free_mib=", " total_mib=")
            .and_then(|value| value.parse::<f64>().ok())
            .map(|mib| (mib.max(0.0) * 1024.0 * 1024.0).round() as u64)
            .ok_or_else(|| format!("invalid Engine device free memory: {line}"))?;
        let memory_total = line
            .split_once(" total_mib=")
            .and_then(|(_, value)| value.trim().parse::<f64>().ok())
            .map(|mib| (mib.max(0.0) * 1024.0 * 1024.0).round() as u64)
            .ok_or_else(|| format!("invalid Engine device total memory: {line}"))?;
        devices.push(EngineDevice {
            index,
            backend,
            name,
            description,
            memory_free,
            memory_total,
        });
    }

    if devices.is_empty() {
        return Err(format!(
            "Engine CLI returned no parseable devices: {}",
            output.trim()
        ));
    }
    Ok(devices)
}

#[cfg(not(target_os = "linux"))]
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

#[cfg(not(target_os = "linux"))]
fn cstr_from_const(ptr: *const c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned()
}

#[cfg(not(target_os = "linux"))]
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

#[cfg(not(any(windows, target_os = "linux")))]
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

#[cfg(all(test, target_os = "linux"))]
mod linux_tests {
    use super::*;

    fn sample_vlm_request(is_image: bool) -> PdfVlmRequest {
        PdfVlmRequest {
            input_path: PathBuf::from(if is_image {
                "/tmp/example.png"
            } else {
                "/tmp/example.pdf"
            }),
            is_image,
            model_path: PathBuf::from("/tmp/model.gguf"),
            mmproj_path: PathBuf::from("/tmp/mmproj.gguf"),
            output_md_path: PathBuf::from("/tmp/output.md"),
            pdfium_lib_path: PathBuf::from("/tmp/libpdfium.so"),
            prompt: "Convert to markdown".to_owned(),
            n_predict: 4096,
            n_ctx: 32768,
            n_batch: 2048,
            n_ubatch: 1024,
            n_parallel: 1,
            n_threads: 8,
            n_threads_batch: 8,
            gpu: Some(0),
        }
    }

    #[test]
    fn image_vlm_cli_args_exclude_pdf_only_pdfium_option() {
        let args = build_pdf_vlm_cli_args(&sample_vlm_request(true));

        assert_eq!(args.first().map(String::as_str), Some("pdfvlm"));
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--image", "/tmp/example.png"])
        );
        assert!(!args.iter().any(|arg| arg == "--pdf"));
        assert!(!args.iter().any(|arg| arg == "--pdfium-lib"));
        assert!(!args.iter().any(|arg| arg == "/tmp/libpdfium.so"));
    }

    #[test]
    fn pdf_vlm_cli_args_keep_pdfium_option() {
        let args = build_pdf_vlm_cli_args(&sample_vlm_request(false));

        assert!(
            args.windows(2)
                .any(|pair| pair == ["--pdf", "/tmp/example.pdf"])
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--pdfium-lib", "/tmp/libpdfium.so"])
        );
        assert!(!args.iter().any(|arg| arg == "--image"));
    }

    #[test]
    fn parses_selected_runtime_cli_device_output() {
        let output = r#"
load_backend: loaded CUDA backend from /opt/openresearchtools/engine/cuda/libggml-cuda.so
devices_count=2
device index=0 backend=CUDA name=CUDA0 desc=NVIDIA GeForce RTX 5090 Laptop GPU type=1 free_mib=23469.0 total_mib=24022.7
device index=1 backend=CPU name=CPU desc=Intel(R) Core(TM) Ultra 9 275HX type=0 free_mib=128221.7 total_mib=128221.7
"#;

        let devices = parse_list_devices_output(output).expect("device output should parse");
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].index, 0);
        assert_eq!(devices[0].backend, "CUDA");
        assert_eq!(devices[0].name, "CUDA0");
        assert_eq!(devices[0].description, "NVIDIA GeForce RTX 5090 Laptop GPU");
        assert_eq!(devices[1].backend, "CPU");
        assert!(devices[0].memory_total > devices[0].memory_free);
    }

    #[test]
    fn rejects_backend_logs_without_device_records() {
        let err = parse_list_devices_output("load_backend: loaded Vulkan backend")
            .expect_err("missing device records should fail");
        assert!(err.contains("no parseable devices"));
    }

    #[test]
    fn linux_vulkan_pdf_e2e_when_fixture_environment_is_set() {
        let Ok(input) = std::env::var("PDF_STUDIO_E2E_INPUT") else { return; };
        let output = std::env::var("PDF_STUDIO_E2E_OUTPUT").expect("PDF_STUDIO_E2E_OUTPUT");
        let runtime = Path::new(crate::app_config::LINUX_VULKAN_RUNTIME_DIR);
        let check = crate::runtime_manager::check_runtime_dir(runtime);
        assert!(check.is_ok(), "missing runtime: {:?}", check.missing);
        let devices = list_bridge_devices(runtime).expect("Vulkan enumeration");
        assert!(devices.iter().any(|device| device.backend.eq_ignore_ascii_case("vulkan")));
        assert!(!devices.iter().any(|device| device.backend.eq_ignore_ascii_case("cuda")));
        run_pdf_fast(runtime, Path::new(&input), Path::new(&output)).expect("app PDF conversion");
        let markdown = std::fs::read_to_string(output).expect("generated Markdown");
        assert!(markdown.contains("ARM64 engine PDF extraction works."), "{markdown}");
    }

    #[test]
    fn installed_linux_backends_are_enumerated_in_separate_processes() {
        let vulkan_root = Path::new("/opt/openresearchtools/engine/vulkan");
        let cuda_root = Path::new("/opt/openresearchtools/engine/cuda");
        if !vulkan_root.join("example-cli").is_file() || !cuda_root.join("example-cli").is_file() {
            return;
        }

        let vulkan = list_bridge_devices(vulkan_root).expect("Vulkan CLI enumeration should work");
        assert!(
            vulkan
                .iter()
                .any(|device| device.backend.eq_ignore_ascii_case("vulkan"))
        );
        assert!(
            !vulkan
                .iter()
                .any(|device| device.backend.eq_ignore_ascii_case("cuda"))
        );

        let cuda = list_bridge_devices(cuda_root).expect("CUDA CLI enumeration should work");
        assert!(
            cuda.iter()
                .any(|device| device.backend.eq_ignore_ascii_case("cuda"))
        );
        assert!(
            !cuda
                .iter()
                .any(|device| device.backend.eq_ignore_ascii_case("vulkan"))
        );
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
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
