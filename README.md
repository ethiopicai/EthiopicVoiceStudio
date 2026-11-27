# Ethiopic Voice Studio

A desktop voice studio focused on Amharic and English. Create speech, transcribe
recordings, and work with audio on your computer—offline after resource setup,
without per-minute speech API fees.

## Voice tools

- **Text to Speech:** synthesize speech using preset voices or your saved reference voices.
- **Transcription:** turn recordings into editable text with word timings and playback highlighting.
- **Dictation:** record speech and review the transcript alongside its audio.
- **Voice Editing — experimental:** change selected words in a recording.
- **Voice Cleanup — experimental:** reduce unwanted noise in recordings.

Saved recordings appear as cards with playback, editable titles and history
controls. Audio and transcripts can move between workspaces.

## Local processing and setup

Studio keeps the desktop app separate from its voice resources. During setup,
it checks your computer and shows which models and speech tools are needed,
their file sizes, and the maximum download size.

Downloads start only after you approve them. You can postpone setup, pause a
download, and continue later. Once the required resources are installed,
supported tools run locally and work offline. Your recordings and transcripts
stay on your computer.

Setup prepares Transcription, Dictation and Text to Speech together. It shows
download, preparation and check steps, and reports **Ready** only after the
required resources pass an offline practice run.

Studio checks installed voice resources at startup and once every 24 hours while
open. It can repair previously approved resources in the background; new or
changed downloads still require your approval. Explicitly paused setup stays paused.

In Settings, **Report a bug** opens the online form with optional name, reply
email and screenshots. App version, operating system and selected tool/model
details are prefilled as JSON; you can review or exclude them.

## App updates

Studio checks for new releases when opened online and shows a notification.
Home keeps a note visible when a newer version is available. In Settings
→ Updates, you can check manually, view the last check time, or turn automatic
checks off. Downloads and installation remain under your control.

Update checks share the app version, platform and a random installation ID for
basic usage counts. You can disable sharing the ID separately. Recordings,
transcripts and file paths are never included in update checks.

## Platforms

- **macOS:** Apple Silicon, macOS 14 or later; the main preview platform.
- **Intel Mac:** macOS 14 or later, interface preview only. Voice-tool setup is
  not available on Intel Macs yet.
- **Windows:** Windows 10 (version 2004 or later) and Windows 11, x64 preview.
  Transcription, Dictation and Text to Speech use tools matched to your computer.

A dedicated GPU is not required. Setup uses compatible GPU tools where available
and CPU speech tools otherwise. Experimental tools may need additional resources.

Features and performance depend on the resources available for your computer.
Voice Editing and Voice Cleanup are experimental.

## Noncommercial preview

EthiopicAI-owned code and assets are available under the PolyForm Noncommercial
License 1.0.0. You may use, modify and redistribute them for permitted
noncommercial purposes. Models, reference voices and third-party components
retain their own terms. See [LICENSE](LICENSE) and
[third-party notices](THIRD_PARTY_NOTICES.md). Separately licensed Amharic
ASR, text-correction and from-scratch speech-generation weights use
[CC BY 4.0](MODEL_LICENSES.md).

For preview announcements, visit [ethiopic.ai](https://ethiopic.ai/).
Questions and bug reports: [contact@ethiopic.ai](mailto:contact@ethiopic.ai).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for contribution and testing guidance.
To bring a new model, follow the [model integration guide](docs/MODEL_INTEGRATION.md)
for adapter interfaces, resource setup, hardware selection and compatibility checks.
