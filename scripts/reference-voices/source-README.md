---
language:
- en
- es
- fr
- de
- it
- pt
- pl
- tr
- ru
- nl
- cs
- ar
- zh
- ja
- hu
- ko
- hi
pipeline_tag: text-to-speech
tags:
- text-to-speech
- tts
- ggml
- vulkan
- c++
- on-device
- llama.cpp
- abbn
license: other
license_name: coqui-public-model-license
license_link: https://coqui.ai/cpml
base_model: coqui/XTTS-v2
widget:
- text: Hello world, this is a test of speech synthesis in English, do you like it ?
  output:
    url: samples/Asya_Anara_en.wav
- text: مَرْحَبًا ، هذا اختبار لتوليد الكلام باللغة العربية؛ هل يعجبك ؟
  output:
    url: samples/Gitta_Nikolina_ar.wav
- text: >-
    Bonjour , ceci est un test de synthèse vocale en français ; est‑ce que vous
    l’aimez  ?
  output:
    url: samples/Vjollca_Johnnie_fr.wav
- text: >-
    Hallo Welt, dies ist ein Test der Sprachsynthese auf Deutsch, gefällt sie
    dir ?
  output:
    url: samples/Dionisio_Schuyler_de.wav
- text: Hola mundo, esto es una prueba de síntesis de voz en español, ¿te gusta ?
  output:
    url: samples/Vjollca_Johnnie_es.wav
- text: Ciao mondo, questo è un test di sintesi vocale in italiano, ti piace ?
  output:
    url: samples/Dionisio_Schuyler_it.wav
- text: 你好世界，这是英语语音合成的测试，你喜欢吗？
  output:
    url: samples/Damien_Black_zh.wav
- text: >-
    Hallo wereld, dit is een test van spraaksynthese in het Nederlands, vind je
    het leuk ?
  output:
    url: samples/Adde_Michal_nl.wav
- text: Ahoj světe, toto je test syntézy řeči v češtině, líbí se ti to ?
  output:
    url: samples/Craig_Gutsy_cs.wav
- text: Helló világ, ez egy teszt a beszédszintézisre magyarul, tetszik ?
  output:
    url: samples/Ludvig_Milivoj_hu.wav
- text: >-
    नमस्ते दुनिया, यह हिन्दी में भाषण संश्लेषण का परीक्षण है, क्या आपको यह पसंद
    है ?
  output:
    url: samples/Nova_Hogarth_hi.wav
- text: Olá mundo, este é um teste de síntese de fala em português, você gosta ?
  output:
    url: samples/Royston_Min_pt.wav
- text: Привет мир, это тест синтеза речи на русском, тебе нравится ?
  output:
    url: samples/Abrahan_Mack_ru.wav
- text: Merhaba dünya, bu bir Türkçe ses sentezi testi; beğeniyor musun ?
  output:
    url: samples/Abrahan_Mack_tr.wav
- text: >-
    Witaj świecie, to jest test syntezy mowy w języku polskim, czy ci się podoba
    ?
  output:
    url: samples/Viktor_Eka_pl.wav
- text: 안녕 세상아, 이것은 한국어 음성 합성 테스트입니다. 마음에 드나요?
  output:
    url: samples/Viktor_Menelaos_ko.wav
- text: こんにちは、世界、これは日本語の音声合成のテストです。気に入りますか？
  output:
    url: samples/Zacharie_Aimilios_ja.wav
---


# ATTS1HG1: High-Performance llama.cpp Implementation of XTTS-v2

**ATTS1HG1 v1.1** is a high-speed, native C++ implementation of the Coqui XTTS-v2 model, utilizing the **llama.cpp** library. It features a custom integrated HiFiGAN vocoder optimized for **Vulkan** and **CPU** inference.

<div align="center">

| **Source Code & GUI** | **Base Model** | **Backend** |
|:---:|:---:|:---:|
| [**GitHub: ATTS1HG1**](https://github.com/abbndz/ATTS1HG1) | [Coqui XTTS-v2](https://huggingface.co/coqui/XTTS-v2) | GGML / Vulkan |

</div>

## 🚀 Key Features

* **Blazing Fast:** Generates audio in **< 0.5s** on consumer GPUs (RTX 3090) and **~1.0s** on CPU.
* **Vulkan Support:** Fully optimized HiFiGAN vocoder running on Vulkan (compatible with NVIDIA, AMD, Intel iGPUs).
* **Lightweight:** Native C++ application utilizing the **llama.cpp**, no heavy Python dependencies.
* **Multi-Language:** Supports 17 languages.
* **Voice :** Supports 58 speaker (similar to XTTS).

## 🌍 Supported Languages

The model supports the following 17 languages:

| Code | Language | Native Name |
| :--- | :--- | :--- |
| **en** | English | English |
| **es** | Spanish | Español |
| **fr** | French | Français |
| **de** | German | Deutsch |
| **it** | Italian | Italiano |
| **pt** | Portuguese | Português |
| **pl** | Polish | Polski |
| **tr** | Turkish | Türkçe |
| **ru** | Russian | Русский |
| **nl** | Dutch | Nederlands |
| **cs** | Czech | Čeština |
| **ar** | Arabic | العربية |
| **zh** | Chinese | 中文 |
| **ja** | Japanese | 日本語 |
| **hu** | Hungarian | Magyar |
| **ko** | Korean | 한국어 |
| **hi** | Hindi | हिन्दी |

## ⚡ Performance

Benchmarks based on standard text generation ("Bonjour le monde") using the C++ client:

| Device | Backend | Latency (Total) | Note |
| :--- | :--- | :--- | :--- |
| **NVIDIA RTX 3090** | **Vulkan** | **~0.47s** | 🚀 **Recommended** |
| **Intel iGPU** | Vulkan | ~1.40s | Good for laptops |
| **CPU (Ryzen/Intel)**| CPU (AVX2)| ~1.02s | Solid fallback |
| **NVIDIA RTX 3090** | CUDA | ~1.45s | Slower on HiFiGAN due to kernel overhead |

> **Note:** The Vulkan backend is significantly faster for the HiFiGAN part of the pipeline compared to CUDA due to optimized command buffers and reduced kernel launch overhead for small convolutions.
>


## 🔧 Key Technical Upgrades

* Split Model Architecture (GPT2 + HiFiGAN):

* The text-to-latent model (GPT2) and the vocoder (HiFiGAN) are now separate GGUF files.

* Allows users to load HiFiGAN via Vulkan for lower latency while keeping GPT2 optimized for CUDA.

* Result: Up to 3x faster audio generation on RTX series.

* Advanced Text Preprocessing:

* Automatic Language Detection (New!):

   ATTS now features a robust LanguageDetector class that analyzes input text.
   Uses Script Range (Unicode blocks), Dictionary (common words), and N-Grams (suffixes/patterns).
   Supports 17 languages with confidence scoring and fallback logic.
   In "Auto Mode" (LangDirIndex == 0), the system automatically selects the language for synthesis.

* **MeCab** Integration: Native Japanese tokenization and segmentation for natural prosody.

* Romanization: Automatic romanization for Chinese (Pinyin), Japanese (Romaji), and Korean (Revised Romanization).

* Num2Words: Converts numbers (e.g., "123") into words ("one hundred twenty-three") across all 17

  

## 🛠️ Usage

This repository contains the converted `.zip` and `.gguf` weights required by the ATTS1HG1 software.

Zero-Configuration Deployment:

1. Download the Pre-compiled GUI Applications (.zip) and  the model files from this repository.

2. No C++ compilation, no Python dependencies, no pip install.

3. Just unzip and run atts_gui_app_v1.1.exe.

4. Load the model in the GUI or CLI and select **Vulkan** for best performance.


## 🧠 Model Files (GGUF via llama.cpp)

* atts1hg1_q6_k.gguf (Legacy: Combined GPT2+HiFiGAN)

* ATTS1_q6_k.gguf (New: GPT2 Only)

* hifigan1_FP16.gguf (New: HiFiGAN Only)



## 🔊 Sample Audio Files

Test the quality with these pre-generated samples covering various speakers and languages.

| Filename | Speaker | Language | Download |
| :--- | :--- | :--- | :--- |
| Vjollca Johnnie | Vjollca Johnnie | French | [⬇️ Download](samples/Vjollca_Johnnie_fr.wav) |
| Gitta Nikolina | Gitta Nikolina | Arabic | [⬇️ Download](samples/Gitta_Nikolina_ar.wav) |
| Damien Black | Damien Black | Chinese | [⬇️ Download](samples/Damien_Black_zh.wav) |
| Asya Anara | Asya Anara | English | [⬇️ Download](samples/Asya_Anara_en.wav) |
| Vjollca Johnnie | Vjollca Johnnie | Spanish | [⬇️ Download](samples/Vjollca_Johnnie_es.wav) |
| Nova Hogarth | Nova Hogarth | Hindi | [⬇️ Download](url:samples/Nova_Hogarth_hi.wav) |
| Adde Michal | Adde Michal | Dutch | [⬇️ Download](samples/Adde_Michal_nl.wav) |
| Craig Gutsy | Craig Gutsy | Czech | [⬇️ Download](samples/Craig_Gutsy_cs.wav) |
| Dionisio Schuyler | Dionisio Schuyler | German | [⬇️ Download](samples/Dionisio_Schuyler_de.wav) |
| Dionisio Schuyler | Dionisio Schuyler | Italian | [⬇️ Download](samples/Dionisio_Schuyler_it.wav) |
| Ludvig Milivoj | Ludvig Milivoj | Hungarian | [⬇️ Download](samples/Ludvig_Milivoj_hu.wav) |
| Royston Min | Royston Min | Portuguese | [⬇️ Download](samples/Royston_Min_pt.wav) |
| Viktor Eka | Viktor Eka | Polish | [⬇️ Download](samples/Viktor_Eka_pl.wav) |
| Abrahan Mack | Abrahan Mack | Russian | [⬇️ Download](samples/Abrahan_Mack_ru.wav) |
| Abrahan Mack | Abrahan Mack | Turkish | [⬇️ Download](samples/Abrahan_Mack_tr.wav) |
| Viktor Menelaos | Viktor Menelaos | Korean | [⬇️ Download](samples/Viktor_Menelaos_ko.wav) |
| Zacharie Aimilios | Zacharie Aimilios | Japanese | [⬇️ Download](samples/Zacharie_Aimilios_ja.wav) |



## 📜 License

This project uses the weights from **Coqui XTTS-v2**, which is licensed under the **Coqui Public Model License (CPML)**.

---
*Credits: Based on the excellent work by Coqui.ai and the GGML library by ggerganov.*