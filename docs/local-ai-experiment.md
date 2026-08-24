# Local AI summarization

The experiment met its initial release gate and ships in `v0.4.0`. Inference remains optional,
local-only, and model-free until the user explicitly chooses a GGUF model.

## Question

Can PdfMerger summarize searchable PDFs locally with useful quality, predictable resource use,
and no loss of its native, private, offline character?

## Constraints

- Inference is local-only. There are no cloud providers, API keys, or document uploads.
- A model is downloaded only after explicit consent, then works offline.
- Model weights are loaded when a summarization job starts and unloaded when it ends.
- NVIDIA CUDA builds target RTX 20, 30, 40, and 50 series GPUs rather than one developer GPU.
- Radeon acceleration through Vulkan is the next hardware experiment.
- macOS means Apple Silicon only. Metal builds use llama.cpp with the same GGUF files as the CPU
  and CUDA backends; Core ML and the Apple Neural Engine remain a separate later evaluation.
- Windows and Linux must retain a CPU baseline.
- Scanned/image-only pages are reported, not sent through hidden OCR.
- PDF text, prompts, and generated summaries are not logged.

## Development stages

1. Extract bounded, page-numbered text and classify pages without searchable text.
2. Exercise the complete job lifecycle with a deterministic mock backend.
3. Benchmark a small curated set of GGUF models through llama.cpp on CPU.
4. Verify multi-generation NVIDIA CUDA support and model-memory release after every job.
5. Enable Apple Silicon Metal builds and verify GPU offload, CPU fallback, performance, and
   model-memory release after every job.
6. Prototype Radeon acceleration through Vulkan, retaining the CPU baseline.
7. Continue improving model installation and removal as the supported model set evolves.

## Measurements

- Summary usefulness and factual consistency on short, long, structured, and multilingual PDFs.
- Preservation of page references through chunking and final synthesis.
- Model load time, time to first token, generation speed, peak RAM, and peak GPU memory.
- Cancellation latency during extraction, loading, and generation.
- Memory retained after successful, failed, and cancelled jobs.
- Installer size and cross-platform build impact without a bundled model.

## Release gate

Local summarization must continue producing useful grounded summaries, fitting supported machines
without disrupting PDF editing, unloading model memory reliably, and keeping release packages
model-free.
