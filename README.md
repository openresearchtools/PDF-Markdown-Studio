# PDF Markdown Studio

![PDF Markdown Studio Demo](Demo.png)
- Windows x64*: [pdf-markdown-studio-windows-x64](https://github.com/openresearchtools/PDF-Markdown-Studio/releases/download/1.0.4/pdf-markdown-studio-windows-x64.exe)
- macOS arm64: [pdf-markdown-studio-macos-arm64.dmg](https://github.com/openresearchtools/PDF-Markdown-Studio/releases/download/1.0.4/pdf-markdown-studio-macos-arm64.dmg)

PDF Markdown Studio is a desktop app for converting PDFs and images into clean Markdown.

It is built as an **example integration client** for OpenResearchTools Engine, showing how to run Engine PDF and VLM workflows from a GUI.

The PDF Markdown Studio application source code is licensed under the MIT License; third-party dependencies and bundled components remain licensed under their respective original licenses.

## What You Can Do

- Add multiple files (PDFs and images) into one workspace.
- Preview the source document and generated Markdown side-by-side.
- Search by text and jump through matching pages.
- Convert selected files using either fast PDF extraction or VLM-based extraction.
- Edit Markdown in place and save changes.

## Supported Files

- PDF (`.pdf`)
- Images (`.png`, `.jpg`, `.jpeg`, `.bmp`, `.gif`, `.webp`, `.tif`, `.tiff`)

## Conversion Modes (How To Choose)

### 1) FAST PDF

Use this for machine-readable digital PDFs.

- Very fast.
- Best when text is selectable in the PDF.
- Output quality is strong for standard digital documents.
- Limitation: table content is often flattened/inline in output; complex tables can become misstructured.

### 2) PDF VLM + Image VLM

Use this when layout is complex, scanned-like, visual-heavy, or FAST output is poor.

- Uses your selected VLM model + MMProj.
- Works for both PDFs and images.
- Slower than FAST, but better for difficult pages.
- Engine applies a per-page quality gate for PDF VLM output. Pages that end up obviously truncated, stuck in repetition/looping, or otherwise fail the gate are retried automatically before final output is written.
- This is meant to reduce bad outputs on difficult pages, but manual inspection is still recommended for important documents.
- Limitation: table quality depends on the selected model's capabilities.
- On complex tables with heavy formatting/whitespace, models can misattribute values to wrong cells or rows.
- If downstream automation depends on table values, compare Markdown against the original document before automated extraction.

### 3) FAST PDF + VLM fallback

Try FAST first, then automatically switch to VLM if FAST identifies non-machine-readable content.

- Good default when PDF quality is mixed.
- Balances speed and robustness.

## Quick Start

1. Open the app.
2. In `Settings`, make sure runtime is healthy and model paths are set.
3. Click `File`, `Add PDFs / Images`.
4. Tick files in the Documents sidebar.
5. Select a conversion mode.
6. Click `Convert Selected`.

Output files are written next to the source document:

- `filenameFAST.md`
- `filenameVLM.md`

## Viewing and Navigation

- Left pane: original PDF/image.
- Right pane: Markdown preview/edit.
- `Find`, `Prev/Next Hit`, and page controls are above the workspace.
- View zoom affects both panes together.

## Runtime and Model Location

Default Engine runtime path:

- Windows: `C:\Users\<user>\AppData\Roaming\OpenResearchTools\PDF Markdown Studio\Engine`
- macOS: `~/Library/Application Support/OpenResearchTools/PDF Markdown Studio/Engine`
- Linux: `~/.local/share/OpenResearchTools/PDF Markdown Studio/Engine`

Default app settings/data path:

- Windows config/data: `C:\Users\<user>\AppData\Roaming\OpenResearchTools\PDF Markdown Studio`
- macOS config/data: `~/Library/Application Support/OpenResearchTools/PDF Markdown Studio`
- Linux config: `~/.config/OpenResearchTools/PDF Markdown Studio`
- Linux data: `~/.local/share/OpenResearchTools/PDF Markdown Studio`

Shared VLM model folder:

- Windows: `C:\Users\<user>\AppData\Roaming\OpenResearchTools\models`
- macOS: `~/Library/Application Support/OpenResearchTools/models`
- Linux: `~/.local/share/OpenResearchTools/models`

Each selected Qwen3.5 family downloads into its own shared repo folder under that global `models` root, including the required MMProj file for the chosen family.

## GPU / CPU Execution

- CPU mode: run without GPU acceleration.
- GPU mode: select one GPU in settings.
- The app sends one selected GPU for VLM execution paths through Engine runtime.

# Troubleshooting
## Unsigned Build Notice

This app is an open-source hobby development effort by the repository owner.
We do not currently have funding for full paid code-signing and notarization
pipelines across all platforms/releases.

Because of that, operating-system protections or hardened security environments
(for example Windows SmartScreen, enterprise endpoint controls, or macOS
Gatekeeper policies) may block unsigned binaries.

If your environment blocks unsigned binaries, the recommended path is:
- build this desktop app from source on the target device,
- build Openresearchtools-Engine from source on the same target device,
- and use those locally-built artifacts in your deployment.

### Windows (when blocked)

- If SmartScreen shows "Windows protected your PC", use `More info` ->
  `Run anyway` only if your policy allows it.
- In the app, go to `Settings -> Runtime Setup` and run:
  - `Download/Repair runtime`
  - `Unblock unsigned runtime`
  - `Recheck`
- The Windows unblock script clears Mark-of-the-Web flags in the selected
  runtime directory by running `Unblock-File` recursively on runtime files.

### macOS (when blocked)

- Try `Right click -> Open` on first launch.
- If blocked by Gatekeeper, use `System Settings -> Privacy & Security ->
  Open Anyway` when available and policy permits.
- In the app, after runtime install/repair, click `Unblock unsigned runtime`
  then `Recheck`.
- The macOS unblock script removes quarantine attributes recursively
  (`xattr -dr com.apple.quarantine`) and restores executable bits for runtime
  binaries/scripts where needed (`chmod +x` on relevant files).

## If conversion fails or setup is incomplete

1. Open `Settings`.
2. Use runtime health/check and download/repair actions.
3. Confirm model and MMProj paths exist.
4. Check `Jobs and logs` for the exact error.

## If adding many files feels slow

- Wait for background imports to finish before converting.
- Large PDFs can take time to rasterize and preview.

## Acknowledgements (What This App Uses)

This app uses [OpenResearchTools Engine](https://github.com/openresearchtools/engine) runtime components for PDF and VLM execution. For this app's active feature set, key upstream technologies include:

- [`Openresearchtools-Engine`](https://github.com/openresearchtools/engine):
  embeddable runtime used by this app (`llama-server-bridge`, runtime orchestration, and model/device execution path).
- [`egui`](https://github.com/emilk/egui) / [`eframe`](https://github.com/emilk/egui/tree/master/crates/eframe):
  native immediate-mode GUI framework used to build this desktop application UI.
- [`llama.cpp`](https://github.com/ggml-org/llama.cpp) and [`ggml`](https://github.com/ggml-org/ggml):
  core inference runtime and device/offload mechanics used through Openresearchtools-Engine.
- [`Docling`](https://github.com/docling-project/docling):
  reference logic for VLM document-conversion behavior used by Engine `pdfvlm`, including page-wise rendering/scaling heuristics (`scale`, `oversample`) and Catmull-Rom style downscale before inference.
- [`PDFium`](https://pdfium.googlesource.com/pdfium/) and [`pdfium-render`](https://github.com/ajrcarey/pdfium-render):
  PDF rasterization/page access primitives used by the app's native PDF rendering and by Engine PDF conversion paths.
## Current VLM Model Lineup

PDF Markdown Studio now uses the `Qwen3.5` GGUF + MMProj model family  [`Qwen`](https://huggingface.co/Qwen):
  upstream Qwen3.5 model family reference used for the app's current vision model lineup for PDF VLM and Image VLM conversion:

- `Qwen3.5 9B` (`Q4_K_M` and `Q8_0`)
- `Qwen3.5 4B` (`Q4_K_M` and `Q8_0`)
- `Qwen3.5 2B` (`Q4_K_M` and `Q8_0`)

The app downloads the text model and the matching MMProj for the selected family automatically into the shared OpenResearchTools model store.

Note the use of the models in our app does **not** imply affiliations or endorsements from original model authors. This is just a personal recommendation after testing many currently available models for speed/quality of the outputs. You are also free to use any other vision model that can run on GGML (llama.cpp backend). The app allows for manual model selection.


Recommended guidance:

- **9B 4-bit is the recommended default** for documents that need higher precision, denser layout understanding, or more reliable structure recovery.
- **2B models often still produce surprisingly strong results** at a fraction of the compute cost, and are a good option when you want speed or need to run on lighter hardware.
- `4B` is the middle ground when you want a better quality/speed balance.

 [`openresearchtools/Qwen3.5-9B-GGUF`](https://huggingface.co/openresearchtools/Qwen3.5-9B-GGUF),
  [`openresearchtools/Qwen3.5-4B-GGUF`](https://huggingface.co/openresearchtools/Qwen3.5-4B-GGUF), and
  [`openresearchtools/Qwen3.5-2B-GGUF`](https://huggingface.co/openresearchtools/Qwen3.5-2B-GGUF):
  converted GGUF + MMProj model repositories used by the app for PDF VLM and Image VLM conversion.
- [`Qwen`](https://huggingface.co/Qwen):
  upstream Qwen3.5 model family reference used for the app's current vision model lineup.

This project is independent and is **not affiliated with, sponsored by, or endorsed by** `egui`, `llama.cpp`, `Docling`, `PDFium`, `Qwen`, or other upstream projects/vendors.



## How to cite

Suggested citation:

Rutkauskas, L. (2026). *PDF Markdown Studio* (Version 1.0.0) [Computer software].
OpenResearchTools. <https://github.com/openresearchtools/pdfmarkdownstudio>.

BibTeX:

```bibtex
@software{Rutkauskas_PDFMarkdownStudio_2026,
  author    = {Rutkauskas, L.},
  title     = {PDF Markdown Studio},
  version   = {1.0.0},
  date      = {2026-03-04},
  url       = {https://github.com/openresearchtools/pdfmarkdownstudio},
  publisher = {OpenResearchTools},
  license   = {MIT}
}
```
