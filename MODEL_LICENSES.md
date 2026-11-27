# EthiopicAI model weights

EthiopicAI makes the following Amharic model weights available under
[Creative Commons Attribution 4.0 International (CC BY 4.0)](https://creativecommons.org/licenses/by/4.0/).
Commercial use is permitted with attribution and compliance with that license.

| Model family | Covered variants |
| --- | --- |
| Amharic ASR and word alignment | `asr-wav2vec_ctc-1b-amh-fp16`, `asr-wav2vec_ctc-1b-amh-int8_onnx`, `asr-wav2vec_ctc-300m-amh-fp16`, `asr-wav2vec_ctc-300m-amh-int8_onnx` |
| Amharic text correction | `textcorrect-char_bart-7m-amh-fp32_onnx`, `textcorrect-char_bart-7m-amh-int8_onnx` |
| Amharic speech generation | `tts-codec_llm-0.6b-amh-fp16`, `tts-codec_llm-0.6b-amh-fp32`, `tts-codec_llm-0.6b-amh-int8_onnx` |

The release, exact files and SHA-256 checksums are identified in
[the model manifest](apps/desktop/resources/studio-manifest.json).
These grants apply to EthiopicAI-owned weights in those variants, including
their use for alignment and speech editing. They replace earlier EthiopicAI
noncommercial declarations for these weights.

Attribution: **EthiopicAI — Amharic speech and text models**,
with the applicable model family and release, a link to
[the model repository](https://huggingface.co/ethiopic/ethiopic-studio-models),
and a link to CC BY 4.0. Indicate modifications when sharing adapted weights.

The speech-generation weights listed above were trained from scratch using the
OmniVoice architecture. They are separate from the upstream OmniVoice pretrained
checkpoint and its weight license. The OmniVoice software library is Apache-2.0.
Meta's upstream Omnilingual ASR code and base models are Apache-2.0; the
EthiopicAI ASR weights above use CC BY 4.0.

Third-party software, tokenizers, audio codecs and reference recordings retain
their own licenses and notices. In particular, this grant does not relicense the
Higgs Audio codec, the separate Pocket TTS model family, or the bundled preview
voice recordings. Their terms are recorded in the manifest and
[third-party notices](THIRD_PARTY_NOTICES.md).

The Studio application's [PolyForm Noncommercial license](LICENSE) is separate
from these model-weight licenses.
