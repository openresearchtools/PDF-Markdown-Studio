# Third-Party Notices

The PDF Markdown Studio application source code is licensed under the MIT License; third-party dependencies and bundled components remain licensed under their respective original licenses.

## App notices

PDF Markdown Studio is a native Rust desktop application that orchestrates local PDF/image loading and OpenResearchTools engine runtime conversion pipelines.

### GUI framework notice

- This app UI is built with `egui/eframe`:
  - https://github.com/emilk/egui
- Non-endorsement:
  - Use of `egui/eframe` in this app does not imply endorsement, sponsorship, or affiliation by the egui project maintainers.

---

## Model notices

### Qwen3-VL model family used by this app

- Download source used by app runtime settings and recommendations:
  - https://huggingface.co/Qwen/Qwen3-VL-8B-Instruct-GGUF
- Upstream model card reference:
  - https://huggingface.co/Qwen/Qwen3-VL-8B-Instruct
- Model card license reference at time of writing (March 3, 2026):
  - `apache-2.0` (as shown on both linked Hugging Face model pages).
- Runtime artifact notice:
  - This app uses GGUF model and MMProj artifacts for local inference compatibility.
- Non-endorsement:
  - Listing/downloading/using these artifacts in this app does not imply endorsement, sponsorship, or affiliation by Qwen or Hugging Face.
- User responsibility:
  - Users are responsible for complying with upstream model licenses and any model-card usage constraints.

---

## Engine notices

### OpenResearchTools engine runtime

- Runtime provenance:
  - https://github.com/openresearchtools/engine
- This GUI calls runtime-provided conversion libraries (`pdf.dll`, `pdfvlm.dll`, `llama-server-bridge`) and does not replace their license obligations.

### PDFium notice

- PDFium is a primary dependency for this app workflow:
  - local PDF rendering/preview
  - PDF page rasterization in conversion paths
  - PDF extraction paths that depend on runtime PDFium linkage
- Runtime lookup path used by this app:
  - `vendor/pdfium/pdfium.dll` (Windows)
  - `vendor/pdfium/libpdfium.dylib` (macOS)
  - `vendor/pdfium/libpdfium.so` (Linux)
- Engine PDFium binary source reference:
  - https://github.com/bblanchon/pdfium-binaries
- Engine third-party license/reference index:
  - https://github.com/openresearchtools/engine/blob/main/third_party/licenses/README.md
- Full PDFium-related and engine license text is bundled in:
  - `ENGINE_THIRD_PARTY_LICENSES_FULL.md`

### GPU runtime notice

- Windows users may choose a CUDA runtime build or a Vulkan runtime build.
- NVIDIA CUDA terms apply only when a Windows CUDA runtime build bundles/uses NVIDIA CUDA runtime binaries (for example, cudart/cublas DLLs).
- Typical CUDA DLLs in CUDA builds: cublas64_13.dll, cublasLt64_13.dll, cudart64_13.dll.
- Official NVIDIA CUDA EULA page: https://docs.nvidia.com/cuda/eula/index.html
- Full runtime-side notices and license text are in:
  - `ENGINE_THIRD_PARTY_LICENSES_FULL.md`
