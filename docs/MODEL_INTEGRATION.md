# Integrating a model

The integration goal is to let a user choose a voice task and work with the same
recording cards, waveform player, timed text, history, and export controls.
Model-specific code belongs behind an adapter. Setup prepares its dependencies;
inference uses verified local files.

**Current boundary:** Studio has task-specific workers and reviewed native dispatch
code. It is not a general plugin loader. Adding a checkpoint or manifest entry
alone does not add a backend. New architectures and runtimes require adapter,
dispatch, packaging, and validation changes. The Python interface sketches below
describe adapter responsibilities; they are not an importable Studio SDK.

## 1. Describe the integration

Include this information in the issue or pull request:

| Item | What to provide |
| --- | --- |
| Task | Transcription/dictation, synthesis, correction, cleanup, or speech editing |
| Identity | Family, release, model version, runtime/precision variants, languages actually tested |
| Rights | Model, code, tokenizer/codec, reference voice, and redistribution terms |
| Assets | Exact file inventory, sizes, SHA-256 values, immutable source revisions, dependencies |
| Runtime | Python/ONNX/PyTorch or another backend; exact platform packages and native dependencies |
| Hardware | Supported OS/architecture, CPU instructions or GPU capability, measured peak RAM/VRAM |
| Behavior | Sample rates, channels, timing support, streaming support, controls, limitations |
| Evidence | Offline inference, startup and warm-job latency, accuracy/quality checks, failure recovery |

CPU support is valuable. Give supported capabilities honestly: a model without
streaming can support transcription and after-stop dictation; it must not be
presented as a live recognizer. A model without word alignment needs a compatible
aligner before it can provide timed-word playback or speech editing.

## 2. Where the changes belong

| Area | Starting point |
| --- | --- |
| Variant schema and dependencies | [`studio.rs`](../crates/resource-packs/src/studio.rs) |
| Hardware selection | [`routing.rs`](../crates/resource-packs/src/routing.rs) and [`hardware_setup.rs`](../apps/desktop/src-tauri/src/hardware_setup.rs) |
| Published model inventory | [`studio-manifest.json`](../apps/desktop/resources/studio-manifest.json) |
| ASR adapter and streaming | [`workers/asr`](../workers/asr) and [`recognition_bundle.rs`](../apps/desktop/src-tauri/src/recognition_bundle.rs) |
| Synthesis and editing | [`workers/tts`](../workers/tts), [`workers/editing`](../workers/editing), [`voice_editing.rs`](../apps/desktop/src-tauri/src/voice_editing.rs) |
| Cleanup | [`workers/cleanup`](../workers/cleanup) and [`cleanup.rs`](../apps/desktop/src-tauri/src/cleanup.rs) |
| Controls and capabilities | [`generation_controls.rs`](../apps/desktop/src-tauri/src/generation_controls.rs) and [`speech_generation.py`](../workers/common/speech_generation.py) |
| Setup and readiness | [`voice_setup.rs`](../apps/desktop/src-tauri/src/voice_setup.rs), [`resource_setup.rs`](../apps/desktop/src-tauri/src/resource_setup.rs), [`model_health.rs`](../apps/desktop/src-tauri/src/model_health.rs) |
| Transcript persistence | [`crates/transcripts`](../crates/transcripts) |
| Model details | [`ModelSettings.svelte`](../apps/desktop/ui/src/ModelSettings.svelte) |

Existing workers contain family-specific assumptions. For example, the CPU TTS
worker manages its backend's temperature and voice cache; the editor adapter
observes its backend's splice events. Implement equivalent behavior inside the
new adapter rather than assuming that a similarly named `synth()` or `edit()`
method is automatically compatible.

## 3. Register assets and select compatible variants

Use **manifest version 2**. These are identity fields from an existing ASR variant;
the full manifest also contains its files, sizes, hashes, dependencies and license:

```json
{
  "id": "asr-wav2vec_ctc-300m-amh-int8_onnx",
  "family": "asr-wav2vec_ctc-300m-amh",
  "release": "asr-wav2vec_ctc-300m-amh-3273h-v1.0",
  "model_version": "v1.0",
  "component": "asr",
  "runtime": "onnxruntime-cpu",
  "precision": "int8 dynamic, per-channel, MatMul/Gemm only (activations fp32)",
  "folder": "asr-wav2vec_ctc-300m-amh-int8_onnx"
}
```

- Variant IDs generally use `task-architecture-size-language-runtime`. Use
  underscores inside a field and hyphens between fields. Keep the family stable
  across runtime variants; display the release in model details. Preserve the
  third-party codec naming/version exceptions already represented in the schema.
- Set `folder` to the variant ID. Keep dependent model folders side by side.
  List exact `files`, their byte sizes and SHA-256 values; `total_bytes` must equal
  their sum. Include the license and measured memory estimates.
- `requires` lists mandatory dependencies. `requires_one_of` lists alternatives
  that selection must resolve to one compatible implementation; do not download
  every alternative. A code-only editor can still require TTS, a codec and an aligner.
- Tier bundle fields include `asr`, `asr_live`, `corrector`, `tts`, `tts_optin`,
  `tts_num_step`, `edit`, and `edit_optin`. Use canonical variant IDs. Default and
  optional selections must respect actual runtime availability as well as hardware.
- Adding a backend requires updating the selector's supported runtime rules and
  the native/worker dispatch. Do not reuse another family's ID or runtime label
  merely to pass these checks.
- For a rename, retain top-level `aliases` and matching per-variant aliases for
  at least one Studio release. Resolve stored IDs and persist canonical IDs.
  Preserve historical inference versions in recordings and corrections. Keep
  migrations scoped to the intended release; never verify an unrelated cached
  release against a different model's hashes.

The current downloader accepts approved first-party Hugging Face locations pinned
to immutable commits. Adding arbitrary hosts or repositories requires an explicit
transport/trust-policy change and review. Runtime catalogs and ASR archive routes
are separate from the variant inventory: update the applicable
[`resources`](../apps/desktop/resources) catalogs and regenerate the master catalog.
Manifest `entry` strings are metadata, not instructions to execute unreviewed code.

Build artifacts outside the repository. Maintain a reproducible file inventory,
lock runtime dependencies, and validate packages on each claimed OS/architecture.
End-user setup must show download sizes and obtain approval, resume interrupted
downloads, verify hashes, prepare a private compatible runtime, and pass local
inference before reporting **Ready**. It must retain an existing working install
until replacement succeeds. Inference must not install packages or fetch weights.

## 4. Model-side contracts

Keep model-facing tensors separate from application storage. Adapters convert
model output to these shared meanings; Rust validates it before committing a card.

| Operation | Input | Output and invariants |
| --- | --- | --- |
| Transcription | Mono float32 audio at 16 kHz, supplied by the host's audio preparation | Text, word spans in seconds, model identity/version; finite chronological spans within the recording |
| Live dictation | Consecutive 16 kHz mono float32 chunks on a worker thread | Append-only final words and a replaceable partial tail; flush remaining words on stop |
| Text to Speech | Prepared text; optional reference audio and reference transcript when required; supported controls and seed | Finite mono float32 samples at a declared rate, then an app-owned PCM WAV and applied-settings metadata |
| Text correction | A batch of strings and an explicit punctuation-only or full-correction mode | Same number/order of strings; punctuation-only must preserve spoken words |
| Cleanup | PCM audio plus its rate/channels | Cleaned audio with the current workflow's rate, channel count and frame count preserved, plus model/processing metadata |
| Alignment | Audio and the transcript being aligned | One word span per transcript token, in recording-relative seconds |
| Speech editing | Original audio/text, replacement text, existing word timings, seed and quality controls | New audio, updated timings and exact replaced input/output frame ranges; untouched PCM stays identical |

The current TTS pipeline writes 24 kHz mono PCM16 WAV. A model with another native
rate needs an explicit validated conversion at the adapter boundary, or coordinated
changes to the writer, playback and validators. Do not label samples with a false
rate. The cleanup worker currently accepts mono/stereo PCM WAV at 8–48 kHz; extend
its validators and tests explicitly for a different contract. Decoding MP3 or other
import formats belongs in audio preparation, not in every model implementation.

### Recognition example

The existing ASR loader returns a recognizer and a streaming factory. Its model
surface can be expressed as:

```python
from typing import Protocol, TypedDict
import numpy as np

class Word(TypedDict):
    word: str
    start: float  # seconds from the recording start
    end: float

class Recognition(TypedDict):
    text: str
    words: list[Word]
    model_version: str

class Recognizer(Protocol):
    version: str

    def transcribe(self, audio: np.ndarray) -> Recognition: ...
    def decode(self, audio: np.ndarray, offset: float = 0.0) -> list[Word]: ...
```

This sketch shows the required fields; adapters may also return validated duration
or provenance fields supported by the worker. `decode()` spans include the supplied
offset. The existing CTC rolling streamer uses that method. A non-CTC backend can
provide its own compatible `feed()`/`finish()` session instead; it must not pretend
to implement CTC decoding.

An ASR result has this application-level shape:

```json
{
  "model": "example-recognizer",
  "model_version": "v1.0",
  "text": "ሰላም",
  "words": [{"word": "ሰላም", "start": 0.12, "end": 0.64}]
}
```

The model label above is illustrative. Use the integration's real identity and
version. Never fabricate evenly spaced timings as if they were measured alignment.
If timings are unavailable, add an aligner or explicitly adapt the UI/storage for
untimed results. The current timed-transcript path expects word timings.

Perform microphone capture separately from inference. Coalesce queued chunks when
inference falls behind, bound the queue, and preserve the recording. Previously
published final words and their spans must never change; the partial tail can be
replaced. Handle overlapping rolling context at that boundary. CPU users can use
after-stop transcription when live latency is unsuitable.

### Synthesis controls and references

Advertise only controls the adapter implements. Current generation capabilities
cover temperature, sentence pause, speed, target duration, generation steps and
seed, with backend-specific defaults/ranges. Extend the native capability mapping
and UI validation together. Reject unsupported or conflicting settings before
loading a model; never silently ignore them. Restore per-job model/RNG state so
settings do not leak into the next generation.

Map the existing voice selector to the model's default voice, saved references,
and imported references. Declare whether a reference transcript is needed. Keep
reference preparation/trimming in Manage voices. Record the actual prepared text,
model/version/revision, reference identity, seed and applied settings with each
audio version. Regenerating a card preserves earlier versions.

### Editing, cleanup and correction

Keep **Fix transcript** separate from **Change what is said**. Text correction
does not silently regenerate audio or mark automated output as human-reviewed.
Use existing timings when available. For long edits, keep the existing bounded
window/context and pause-selection behavior; re-align the edited window and shift
later timestamps by its duration change.

An editor adapter must expose exact splice information, not just an approximate
changed interval on a fully re-encoded recording. The current engine observes
backend splice events, preserves original PCM outside those events, and stores
both time and integer frame coordinates. Reuse that orchestration or implement
and test an equivalent adapter. Keep undo, permanent synthetic-region marks,
seed-based retries, comparison playback, and the edit log in exports.

Cleanup should create a new owned result and preserve the original. A denoiser
used before ASR is a separate integration: compensate any lookahead/resampling
delay, keep timestamps on the original recording's clock, preserve source audio,
and record the preprocessing provenance. Do not automatically reuse stale timings
after a processor changes duration or speech content.

## 5. Worker transport and lifecycle

Current worker channels are private UTF-8 JSON lines over stdin/stdout, plus
validated app-owned audio/result files. They are not a public HTTP API. Follow the
native caller's existing schema and bounds. The protocols differ by task:

| Worker | Requests | Completion / failures |
| --- | --- | --- |
| Resident ASR | `type: transcribe` with `audio`/`output`; or `type: dictation`, subsequent `audio` samples and `finish` | `protocol: 1` stage/transcript events; atomically written result; `finished` followed by `idle`; errors use the existing string codes |
| Synthesis/editing | Correlated `request` token, `action`, project/revision and task fields | `protocol: 1`, matching `request`, then `stage`, `result`, or `error: {kind, message}` |
| Cleanup | CLI audio/output/metadata paths | `protocol: 1` stage events, atomically written WAV/metadata and process exit status |

For example, a synthesis request to the existing project worker is:

```json
{
  "request": 7,
  "action": "synthesize",
  "projectId": "demo-project",
  "revision": 0,
  "text": "ሰላም ጤና ይስጥልኝ።",
  "referenceText": "",
  "seed": 42,
  "generationOptions": {"temperature": 0.3, "sentencePause": 0.2}
}
```

`demo-project` must already exist in app-owned project storage. An empty reference
transcript is valid only for a backend that does not require one. Return the actual
settings and output metadata; the native layer validates and enriches provenance
before saving. Do not add a new universal request envelope without changing both
ends and their protocol tests.

- Load a model once for compatible jobs. Key residency by the model/runtime/device
  identity; serialize stateful inference. Keep shared TTS/editor dependencies in
  one instance where possible. Coordinate GPU work through the native queue/locks.
- Follow the existing keep-ready/unload preferences and constrained-hardware
  offloading behavior. Do not add an arbitrary blanket RAM requirement.
- Keep stdout exclusively for protocol messages. Transcript text belongs only in
  intended result/transcript messages, not diagnostic logs. Do not log recordings,
  scripts, transcripts, local paths, or credentials through model libraries.
- Preserve bounded queues and parent-loss handling. Current cancellation may stop
  the child process; leave committed history intact and discard incomplete outputs.
  Commit a WAV/metadata result atomically and never overwrite an original recording.
- Use real progress when measurable and an indeterminate stage otherwise. Persist
  job state in the native service so navigating between workspaces does not lose it.
- Map failures to short actionable messages: unavailable tools, unsupported input,
  memory exhaustion, bad alignment, or interrupted processing. New error kinds need
  native mappings and tests. Do not expose raw Python tracebacks in the UI.
- Run offline after setup. Disable library telemetry and accidental hub fetches;
  include every required dependency. Python offline guards do not make unreviewed
  provider code safe to execute. Review model loaders and any executable model files.

## 6. Acceptance checklist

- [ ] Validate the manifest/file inventory and regenerate/check the master catalog.
- [ ] Test tier selection, unsupported hardware and dependency alternatives. An
  existing tier name does not prove that a new runtime supports its GPUs.
- [ ] Test approved fresh setup: models **and** runtime **and** real offline
  inference before Ready. Test pause/resume, network loss, corrupt files, failed
  preparation and recovery to the previous working install.
- [ ] Test upgrade with old IDs, settings, download state and caches. Preserve
  unchanged weights, existing recordings, human corrections and historical versions.
- [ ] Test the model-side contracts: finite samples/timings, offsets, unsupported
  controls, reference requirements and empty/invalid input.
- [ ] Test repeated jobs, model retention/unload, cancellation, parent exit and
  recovery without memory leaks or accidental model reloads.
- [ ] Test streaming final/partial semantics and a slow worker if live input is supported.
- [ ] For editing, prove byte-identical untouched PCM, correct timestamp shifts,
  long recordings, retry, undo, and audit export.
- [ ] Test playback, word highlighting, corrections, card regeneration, handoff
  between workspaces, history and deletion using the existing shared components.
- [ ] Run real native tests on each claimed platform/runtime. Record the hardware,
  precision, versions, latency, peak memory and results. Identify emulated policies
  and untested devices explicitly; do not present them as physical qualification.
- [ ] Include licenses/notices and clean model documentation. Keep weights, private
  environments, test recordings, credentials and generated results out of Git.

Use the existing [validation tools](../scripts) and nearby tests as starting
points. Publishing models, changing download trust, and distributing signed app
releases remain separate maintainer actions after the integration is reviewed.
