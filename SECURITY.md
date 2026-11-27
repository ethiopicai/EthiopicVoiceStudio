# Security

Studio is preview software. Report suspected security vulnerabilities privately
to **contact@ethiopic.ai**. Include the application version, operating system
and steps to reproduce. Do not post credentials, private recordings, personal
information or exploit payloads in a public issue.

## Local data and downloads

Audio, transcripts, corrections and reference profiles are stored locally.
Update checks send the app version, platform and an optional random installation
ID; Settings provides controls for automatic checks and usage statistics.
Bug reports include a reviewable, limited diagnostics object. Screenshots and
text that you attach may contain personal information; review them before sending.

Voice resources use approved HTTPS locations, immutable revisions and SHA-256
checksums. Setup checks archive paths and verifies resources before activating
them. Studio does not provide a loader for arbitrary model checkpoints or Python
plugins. Downloaded model code and native runtimes are trusted executable
software; they are not isolated by a security sandbox.

## Dependency maintenance

The app and optional voice runtimes have separate dependency inventories.
The current runtime packs include PyTorch 2.9.0/2.9.1 and Python packaging tools
with published upstream advisories. Relevant examples include the PyTorch
checkpoint loader ([GHSA-63cw-57p8-fm3p](https://github.com/advisories/GHSA-63cw-57p8-fm3p))
and local tensor/JIT interfaces
([GHSA-qfhq-4f3w-5fph](https://github.com/advisories/GHSA-qfhq-4f3w-5fph),
[GHSA-rrmf-rvhw-rf47](https://github.com/advisories/GHSA-rrmf-rvhw-rf47)).
Studio uses verified model files and fixed worker interfaces rather than
accepting arbitrary checkpoints, model code or JIT programs from recordings.
This limits exposure but does not remove upstream vulnerabilities. Runtime
upgrades require platform and inference qualification; a checksum verifies
identity, not absence of vulnerabilities. Do not use an unreviewed replacement
runtime or checkpoint.

Preview installers may be unsigned or not notarized. Use only the downloads
linked from the EthiopicAI website or this repository's Releases, and verify
published checksums. Signed production distribution remains a separate release
qualification step.
