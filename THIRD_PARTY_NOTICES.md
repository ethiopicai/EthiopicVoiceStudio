# Third-party material

The root PolyForm Noncommercial license applies to the Studio application.
The separately licensed EthiopicAI model weights listed in
[MODEL_LICENSES.md](MODEL_LICENSES.md) use CC BY 4.0. Existing third-party
licenses and notices remain applicable independently.

| Material | Attribution and license location |
| --- | --- |
| Rust and JavaScript dependencies | Exact versions are pinned in `Cargo.lock`, `package-lock.json`, and `apps/desktop/ui/package-lock.json`. Each dependency retains its upstream license. |
| Symphonia audio decoding | MPL-2.0 notice is included in `apps/desktop/ui/src/symphonia-license.txt`. |
| Amharic Text Processor subset | Upstream MIT declaration, author, revision, and unmodified module list are recorded in `third_party/amharic-text-processor/README.studio.md` and its original `pyproject.toml`. |
| Inworld optional denoiser | Notice is included in `apps/desktop/ui/src/noise-reduction-license.txt`; downloaded weights retain their upstream terms. |
| Mamba reference implementation | Apache-2.0 text is included in `workers/cleanup/licenses/mamba-Apache-2.0.txt`. |
| Preview reference audio | `apps/desktop/ui/public/reference-voices/preview-v3/LICENSES.txt` contains the source attribution and full Coqui Public Model License. The synthesized preview clips are noncommercial. |
| Fictional persona portraits | AI-generated fictional profile illustrations. Display-name mappings are in `scripts/reference-voices/personas.json`; these portraits do not depict or identify source speakers. |
| Fonts | Font notices included alongside bundled font assets in the UI public directory must be preserved. |
| Downloadable models | Per-variant licenses are recorded in `apps/desktop/resources/studio-manifest.json` and the public model cards at https://huggingface.co/ethiopic/ethiopic-studio-models. Weights are not included in this repository. |
| Downloadable Python runtimes | CPython and installed speech-library licenses remain applicable. Runtime packs preserve the Python license and package license files. |
| Microsoft Visual C++ runtime | Windows voice runtimes include app-local redistributable libraries. Source, version, checksums and notices are included under `runtime/licenses/visual-cpp` in the downloaded pack. Microsoft license terms apply. |

When redistributing the app or a runtime, preserve the applicable licenses,
attributions and notices. The root license does not grant additional rights to
third-party software, models or reference voices.
