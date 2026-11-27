# Contributing to Ethiopic Voice Studio

Contributions can improve speech quality, accessibility, Amharic and English text
handling, performance on modest computers, setup, or the shared editing UI.
Model integrations are welcome: start with the [model integration guide](docs/MODEL_INTEGRATION.md).

## Before starting

- Read the [README](README.md), [LICENSE](LICENSE), and
  [third-party notices](THIRD_PARTY_NOTICES.md). EthiopicAI-owned code and assets
  use the PolyForm Noncommercial License 1.0.0, which permits noncommercial use,
  modification and redistribution. Third-party models, datasets and voices keep
  their own terms; contributing does not grant additional rights to them.
- For a substantial change, open an issue describing the user problem, proposed
  behavior, supported platforms, and relevant model or dependency licenses.
- Keep each pull request focused. Include a small reproducible example and
  explain how it was tested. UI changes should include a screenshot or short
  recording and checks at narrow and wide window sizes.
- Use synthetic, licensed, or explicitly consented test audio. Remove personal
  information from fixtures, screenshots, and logs.

## Working locally

The desktop app uses Rust/Tauri and Svelte. Current CI uses stable Rust, Node.js
22, and Python 3.12 for release tooling. Python inference environments are packaged
separately and must use the versions and dependencies qualified for their backend.
Native builds also need the target platform's Tauri prerequisites and toolchain;
the supported app targets are Apple Silicon macOS and Windows x64.

From the repository root:

```sh
npm ci
npm ci --prefix apps/desktop/ui
npm run dev
```

Develop ordinary interface changes without downloading model weights. A real
model integration also needs its verified local assets and compatible runtime.
End users should not need a developer environment, Python installation, terminal,
or Hugging Face account to use an accepted integration.

## Checks

The **Desktop preview packages** GitHub workflow builds separate Apple Silicon
Mac, Intel Mac and Windows x64 installers. Run it manually for downloadable Actions
artifacts. Enable **create_draft**, or push a version tag matching the app version,
to attach the checked installers, SHA-256 checksums and metadata to a draft
prerelease. Publication stays manual. Intel packages are for interface testing;
they do not include qualified voice-tool runtimes. These preview packages do not
replace production signing, notarization and acceptance checks.

Run the checks relevant to your change; CI defines the complete required set in
[`.github/workflows`](.github/workflows).

```sh
cargo fmt --all --check
cargo test --workspace --locked -- --test-threads=1
npm run test:ui
npm run build:ui
npm run test:release-tools
```

For model/catalog changes, also run:

```sh
python3 scripts/check-model-names.py
npm run catalog:check
```

Regenerate the master catalog with `npm run catalog:build` if a provider catalog
changes. Add functional tests for changed behavior, including failure and recovery
paths. Native tests run serially because some exercise process-wide worker pools.
Tests marked `ignored` require explicit local model/runtime fixtures; do not
enable them against someone's ordinary app storage. Report which real model and
hardware checks were run, and which were not.

## Pull request handoff

Describe the problem, resulting behavior, checks, and any limitations. For a model,
include the information and acceptance checks in the integration guide. Separate
measured results from estimates and reference-provider tests from native app tests.

Keep model weights, runtime environments, credentials, recordings, generated
outputs, and build artifacts out of Git. Model and runtime publication is handled
separately by maintainers after review. Never change published artifacts in place
or remove an older model release as part of an ordinary contribution.

Model development uses a branch in a **separate private repository**; branches in
a public repository are public too. The model publisher stages privately by
default. Public promotion requires a reviewed staging commit, matching file
inventories and a clean public-text scan. Promote files into the public history;
never merge private development history into it.
