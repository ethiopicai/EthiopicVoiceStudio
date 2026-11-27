//! Local, source-aware recording handoffs. Original files are never moved or modified.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};
use tauri::Manager;
const AUDIO_LIMIT: u64 = 100 * 1024 * 1024;
const CONTEXT_LIMIT: usize = 8 * 1024 * 1024;
pub const CONTEXT_FILE: &str = "source-media.json";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Reference {
    pub kind: String,
    pub id: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Word {
    pub word: String,
    pub start: f64,
    pub end: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Context {
    pub schema_version: u8,
    pub source: Reference,
    pub audio_sha256: String,
    pub text: String,
    pub words: Vec<Word>,
    pub timings_current: bool,
    pub model_version: Option<String>,
    pub transcript: Option<transcripts::Transcript>,
    #[serde(default)]
    pub transcript_history: Vec<transcripts::Transcript>,
    pub transcript_audio_sha256: Option<String>,
    pub edit_log: Option<Value>,
    pub cleanup: Option<Value>,
    #[serde(default)]
    pub edit_history: Vec<Value>,
    #[serde(default)]
    pub cleanup_history: Vec<Value>,
    pub edited_regions: Vec<Value>,
    pub lineage: Vec<Reference>,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Input {
    Transcript {
        id: String,
    },
    Recording {
        id: String,
    },
    CleanupRecording {
        id: String,
    },
    Cleanup {
        id: String,
        #[serde(default)]
        original: bool,
    },
    Editing {
        id: String,
        #[serde(default)]
        original: bool,
    },
    Synthesis {
        id: String,
    },
    Path {
        path: String,
    },
    File {
        name: String,
        bytes: Vec<u8>,
        context: Option<Box<Context>>,
        text: Option<String>,
    },
}
#[derive(Serialize)]
pub struct Package {
    pub name: String,
    pub bytes: Vec<u8>,
    pub context: Context,
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn valid_id(id: &str) -> Result<(), String> {
    if id.is_empty()
        || id.len() > 100
        || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err("This recording reference is invalid.".into());
    }
    Ok(())
}
fn valid_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.len() > 240
        || name.contains(['/', '\\'])
        || name.chars().any(char::is_control)
        || !name.to_ascii_lowercase().ends_with(".wav")
    {
        return Err("Choose a local WAV recording.".into());
    }
    Ok(())
}
fn managed(root: &Path, relative: &Path) -> Result<PathBuf, String> {
    if relative.is_absolute()
        || relative
            .components()
            .any(|v| !matches!(v, std::path::Component::Normal(_)))
    {
        return Err("Invalid recording reference.".into());
    }
    let base = root
        .canonicalize()
        .map_err(|_| "Studio storage is unavailable")?;
    let path = root.join(relative);
    let info = fs::symlink_metadata(&path)
        .map_err(|_| "This recording is no longer available. Refresh its source card.")?;
    if !info.is_file()
        || info.file_type().is_symlink()
        || path.canonicalize().map_err(|_| "Recording unavailable")? != base.join(relative)
    {
        return Err("Redirected recording storage is not supported.".into());
    }
    Ok(path)
}
fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let info = fs::symlink_metadata(path).map_err(|_| "This recording is no longer available.")?;
    if !info.is_file() || info.file_type().is_symlink() || info.len() > limit {
        return Err("Choose a regular audio file up to 100 MB.".into());
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut bytes = Vec::new();
    options
        .open(path)
        .map_err(|_| "The recording could not be opened. Check the file path.")?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "The recording could not be read")?;
    if bytes.len() as u64 > limit {
        return Err("The recording is too large.".into());
    }
    Ok(bytes)
}
fn read_json(root: &Path, relative: &Path) -> Result<Value, String> {
    let p = managed(root, relative)?;
    serde_json::from_slice(&read_bounded(&p, 16 * 1024 * 1024)?)
        .map_err(|_| "Recording metadata could not be read".into())
}
pub fn validate_context(context: &Context, bytes: &[u8]) -> Result<(), String> {
    if context.lineage.len() > 32
        || context.transcript_history.len() > 32
        || context.edit_history.len() > 32
        || context.cleanup_history.len() > 32
    {
        return Err("This recording's transfer history is too large. Export a fresh WAV to start a new project.".into());
    }
    if serde_json::to_vec(context)
        .map_err(|_| "Recording metadata is invalid")?
        .len()
        > CONTEXT_LIMIT
        || context.schema_version != 1
        || context.audio_sha256 != hash(bytes)
        || context.text.len() > 200000
        || context.words.len() > 100000
    {
        return Err("Recording metadata does not match this audio.".into());
    }
    valid_id(&context.source.id)?;
    let wav = hound::WavReader::new(std::io::Cursor::new(bytes))
        .map_err(|_| "Choose a PCM WAV recording")?;
    let duration = wav.duration() as f64 / wav.spec().sample_rate as f64;
    for region in &context.edited_regions {
        let span = region["span_out"]
            .as_array()
            .filter(|a| a.len() == 2)
            .ok_or("Edited-region metadata is invalid")?;
        let (start, end) = (
            span[0]
                .as_f64()
                .ok_or("Edited-region metadata is invalid")?,
            span[1]
                .as_f64()
                .ok_or("Edited-region metadata is invalid")?,
        );
        if !start.is_finite()
            || !end.is_finite()
            || start < 0.
            || end < start
            || end > duration + 0.05
        {
            return Err("Edited regions do not match this recording.".into());
        }
    }
    let mut last = 0.;
    for word in &context.words {
        if word.word.trim().is_empty()
            || word.word.len() > 4096
            || !word.start.is_finite()
            || !word.end.is_finite()
            || word.start < last
            || word.end < word.start
            || word.end > duration + 0.05
        {
            return Err("Word timings do not match the recording.".into());
        }
        last = word.end;
    }
    Ok(())
}
pub fn write_context(
    directory: &Path,
    context: Option<&Context>,
    bytes: &[u8],
) -> Result<(), String> {
    if let Some(context) = context {
        validate_context(context, bytes)?;
        fs::write(
            directory.join(CONTEXT_FILE),
            serde_json::to_vec(context).map_err(|_| "Recording metadata is invalid")?,
        )
        .map_err(|_| "Could not save recording details. Check storage.")?;
    }
    Ok(())
}
pub fn read_context(directory: &Path, bytes: &[u8]) -> Result<Option<Context>, String> {
    let file = directory.join(CONTEXT_FILE);
    if !file
        .try_exists()
        .map_err(|_| "Recording details unavailable")?
    {
        return Ok(None);
    }
    let context: Context = serde_json::from_slice(&read_bounded(&file, CONTEXT_LIMIT as u64)?)
        .map_err(|_| "Recording details could not be read")?;
    validate_context(&context, bytes)?;
    Ok(Some(context))
}
pub fn inherit(source: &Path, destination: &Path, bytes: &[u8]) -> Result<(), String> {
    write_context(destination, read_context(source, bytes)?.as_ref(), bytes)
}
fn base(bytes: &[u8], source: Reference) -> Context {
    Context {
        schema_version: 1,
        source,
        audio_sha256: hash(bytes),
        text: String::new(),
        words: vec![],
        timings_current: false,
        model_version: None,
        transcript: None,
        transcript_history: vec![],
        transcript_audio_sha256: None,
        edit_log: None,
        cleanup: None,
        edit_history: vec![],
        cleanup_history: vec![],
        edited_regions: vec![],
        lineage: vec![],
    }
}
fn advance(mut context: Context, source: Reference, bytes: &[u8]) -> Context {
    if context.source.kind != source.kind || context.source.id != source.id {
        context.lineage.push(context.source.clone());
    }
    context.source = source;
    context.audio_sha256 = hash(bytes);
    context
}
fn words(value: &Value) -> Result<Vec<Word>, String> {
    serde_json::from_value(value.clone())
        .map_err(|_| "The saved word timings could not be transferred.".into())
}
fn owned_audio(root: &Path, relative: &Path) -> Result<Vec<u8>, String> {
    let bytes = read_bounded(&managed(root, relative)?, AUDIO_LIMIT)?;
    crate::asr::validate_audio(&bytes)?;
    Ok(bytes)
}
pub fn resolve(root: &Path, input: Input) -> Result<Package, String> {
    let (name, bytes, context) = match input {
        Input::File {
            name,
            bytes,
            context,
            text,
        } => {
            let (name, bytes) = crate::audio_import::prepare(name, bytes)?;
            crate::asr::validate_audio(&bytes)?;
            let mut context = context.map(|value| *value).unwrap_or_else(|| {
                base(
                    &bytes,
                    Reference {
                        kind: "file".into(),
                        id: hash(&bytes),
                    },
                )
            });
            if let Some(text) = text {
                if text != context.text {
                    context.text = text;
                    context.timings_current = false;
                }
            }
            (name, bytes, context)
        }
        Input::Path { path } => return resolve_path(root, &path),
        Input::Transcript { id } => {
            valid_id(&id)?;
            let store = transcripts::TranscriptStore::new(root.join("transcripts"))
                .map_err(|_| "Transcript storage unavailable")?;
            let transcript = store
                .get(&id)
                .map_err(|_| "This transcript is no longer available")?;
            let rel = PathBuf::from("transcripts").join(&id);
            let bytes = owned_audio(root, &rel.join("audio.wav"))?;
            let mut context = read_context(&root.join(&rel), &bytes)?.unwrap_or_else(|| {
                base(
                    &bytes,
                    Reference {
                        kind: "transcript".into(),
                        id: id.clone(),
                    },
                )
            });
            context = advance(
                context,
                Reference {
                    kind: "transcript".into(),
                    id,
                },
                &bytes,
            );
            context.text = transcript.text.clone();
            context.words = transcript
                .words
                .iter()
                .filter(|w| !w.corrected.trim().is_empty())
                .map(|w| Word {
                    word: w.corrected.clone(),
                    start: w.start,
                    end: w.end,
                })
                .collect();
            if let Some(previous) = context.transcript.take() {
                if previous.id != transcript.id || previous.revision != transcript.revision {
                    context.transcript_history.push(previous);
                }
            }
            if context.transcript_history.len() > 32 {
                return Err("This recording's transfer history is too large. Export a fresh WAV to continue.".into());
            }
            context.timings_current = !context.words.is_empty()
                && context
                    .words
                    .iter()
                    .all(|w| w.word.split_whitespace().count() == 1);
            context.model_version = Some(transcript.model_version.clone());
            context.transcript_audio_sha256 = Some(hash(&bytes));
            context.transcript = Some(transcript.clone());
            (transcript.name, bytes, context)
        }
        Input::Recording { id } => return resolve_draft(root, id, true),
        Input::CleanupRecording { id } => return resolve_draft(root, id, false),
        Input::Cleanup { id, original } => {
            valid_id(&id)?;
            let rel = PathBuf::from("cleanup-results").join(&id);
            let result = read_json(root, &rel.join("result.json"))?;
            let name = result["name"]
                .as_str()
                .ok_or("Recording name unavailable")?
                .to_owned();
            valid_name(&name)?;
            let source_bytes = owned_audio(root, &rel.join("original.wav"))?;
            let mut context = read_context(&root.join(&rel), &source_bytes)?.unwrap_or_else(|| {
                base(
                    &source_bytes,
                    Reference {
                        kind: "cleanup-original".into(),
                        id: id.clone(),
                    },
                )
            });
            let bytes = if original {
                source_bytes
            } else {
                owned_audio(root, &rel.join("cleaned.wav"))?
            };
            if !original {
                context.timings_current = false;
                if let Some(previous) = context.cleanup.replace(result) {
                    context.cleanup_history.push(previous);
                }
            }
            let context = advance(
                context,
                Reference {
                    kind: if original {
                        "cleanup-original"
                    } else {
                        "cleanup"
                    }
                    .into(),
                    id,
                },
                &bytes,
            );
            (name, bytes, context)
        }
        Input::Editing { id, original } => resolve_editing(root, id, original, false, None)?,
        Input::Synthesis { id } => resolve_editing(root, id, false, true, None)?,
    };
    valid_name(&name)?;
    validate_context(&context, &bytes)?;
    Ok(Package {
        name,
        bytes,
        context,
    })
}
fn resolve_draft(root: &Path, id: String, is_asr: bool) -> Result<Package, String> {
    valid_id(&id)?;
    let category = if is_asr {
        "recordings"
    } else {
        "cleanup-recordings"
    };
    let rel = PathBuf::from(category).join(&id);
    let name: String = serde_json::from_value(read_json(root, &rel.join("name.json"))?)
        .map_err(|_| "Recording name unavailable")?;
    valid_name(&name)?;
    let file = if is_asr { name.as_str() } else { "audio.wav" };
    let bytes = owned_audio(root, &rel.join(file))?;
    let context = read_context(&root.join(&rel), &bytes)?.unwrap_or_else(|| {
        base(
            &bytes,
            Reference {
                kind: if is_asr {
                    "recording"
                } else {
                    "cleanup-recording"
                }
                .into(),
                id: id.clone(),
            },
        )
    });
    let context = advance(
        context,
        Reference {
            kind: if is_asr {
                "recording"
            } else {
                "cleanup-recording"
            }
            .into(),
            id,
        },
        &bytes,
    );
    validate_context(&context, &bytes)?;
    Ok(Package {
        name,
        bytes,
        context,
    })
}
fn resolve_editing(
    root: &Path,
    id: String,
    original: bool,
    synthesis: bool,
    version_override: Option<usize>,
) -> Result<(String, Vec<u8>, Context), String> {
    valid_id(&id)?;
    let rel = PathBuf::from("voice-edit/projects").join(&id);
    let mut project = read_json(root, &rel.join("project.json"))?;
    let head = if original {
        0
    } else if let Some(version) = version_override {
        version
    } else {
        project["head"]
            .as_u64()
            .ok_or("Project revision unavailable")? as usize
    };
    let version = project["versions"]
        .get(head)
        .ok_or("Project revision unavailable")?
        .clone();
    project["head"] = json!(head);
    let name = project["name"]
        .as_str()
        .unwrap_or("Recording.wav")
        .to_owned();
    let name = if name.to_ascii_lowercase().ends_with(".wav") {
        name
    } else {
        format!("{name}.wav")
    };
    valid_name(&name)?;
    let original_bytes = owned_audio(root, &rel.join("original.wav"))?;
    let mut context = read_context(&root.join(&rel), &original_bytes)?.unwrap_or_else(|| {
        base(
            &original_bytes,
            Reference {
                kind: "editing".into(),
                id: id.clone(),
            },
        )
    });
    let file = if synthesis {
        "synthesis.wav"
    } else {
        version["audio"]
            .as_str()
            .ok_or("Project audio unavailable")?
    };
    valid_name(file)?;
    let bytes = owned_audio(root, &rel.join(file))?;
    if synthesis {
        let info = read_json(root, &rel.join("synthesis-context.json"))?;
        context.text = info["text"].as_str().unwrap_or_default().into();
        context.words = vec![];
        context.timings_current = false;
        if let Some(previous) = context.edit_log.replace(info) {
            context.edit_history.push(previous);
        }
        context.model_version = None;
        let wav = hound::WavReader::new(std::io::Cursor::new(&bytes))
            .map_err(|_| "Generated audio unavailable")?;
        context.edited_regions = vec![
            json!({"span_in":[0.,wav.duration() as f64/wav.spec().sample_rate as f64],"span_out":[0.,wav.duration() as f64/wav.spec().sample_rate as f64],"kind":"synthesis"}),
        ];
    } else {
        context.text = version["text"].as_str().unwrap_or_default().into();
        context.words = words(&version["words"])?;
        context.timings_current = !context.words.is_empty();
        context.edited_regions = version["regions"].as_array().cloned().unwrap_or_default();
        if let Some(object) = project.as_object_mut() {
            object.remove("sourceMedia");
        }
        if context.edit_log.as_ref() != Some(&project) {
            if let Some(previous) = context.edit_log.replace(project) {
                context.edit_history.push(previous);
            }
        }
    }
    context = advance(
        context,
        Reference {
            kind: if synthesis {
                "synthesis"
            } else if original {
                "editing-original"
            } else {
                "editing"
            }
            .into(),
            id,
        },
        &bytes,
    );
    Ok((name, bytes, context))
}
fn resolve_path(root: &Path, value: &str) -> Result<Package, String> {
    let value = value.trim();
    let value = if value.len() > 1
        && ((value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with('\'') && value.ends_with('\'')))
    {
        &value[1..value.len() - 1]
    } else {
        value
    };
    if value.len() > 32768
        || value.chars().any(char::is_control)
        || value.starts_with("\\\\")
        || value.starts_with("//")
    {
        return Err("Paste a local audio file path.".into());
    }
    let path = if value.starts_with("file:") {
        let url = tauri::Url::parse(value).map_err(|_| "Paste a local audio file path")?;
        if url.host_str().is_some_and(|host| host != "localhost") {
            return Err("Use a local audio file, not a network address.".into());
        }
        url.to_file_path()
            .map_err(|_| "Paste a local audio file path")?
    } else {
        PathBuf::from(value)
    };
    if !path.is_absolute() {
        return Err("Paste the full local audio path, including its folder.".into());
    }
    let name = path
        .file_name()
        .and_then(|v| v.to_str())
        .ok_or("Recording filename unavailable")?
        .to_owned();
    crate::audio_import::extension(&name)?;
    let canonical = path
        .canonicalize()
        .map_err(|_| "This audio path could not be opened. Check that the file still exists.")?;
    if fs::symlink_metadata(&path)
        .map_err(|_| "Recording unavailable")?
        .file_type()
        .is_symlink()
    {
        return Err("Choose the recording file itself rather than a symbolic link.".into());
    }
    if let Ok(base) = root.canonicalize() {
        if let Ok(relative) = canonical.strip_prefix(base) {
            let parts = relative
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            let input = match parts.as_slice() {
                [kind, id, file] if kind == "transcripts" && file == "audio.wav" => {
                    Some(Input::Transcript { id: id.clone() })
                }
                [kind, id, file] if kind == "recordings" => {
                    let stored = read_json(
                        root,
                        &PathBuf::from("recordings").join(id).join("name.json"),
                    )?;
                    (stored.as_str() == Some(file.as_str()))
                        .then(|| Input::Recording { id: id.clone() })
                }
                [kind, id, file] if kind == "cleanup-recordings" && file == "audio.wav" => {
                    Some(Input::CleanupRecording { id: id.clone() })
                }
                [kind, id, file]
                    if kind == "cleanup-results"
                        && matches!(file.as_str(), "cleaned.wav" | "original.wav") =>
                {
                    Some(Input::Cleanup {
                        id: id.clone(),
                        original: file == "original.wav",
                    })
                }
                [area, kind, id, file] if area == "voice-edit" && kind == "projects" => {
                    if file == "synthesis.wav" {
                        Some(Input::Synthesis { id: id.clone() })
                    } else {
                        let project = read_json(
                            root,
                            &PathBuf::from("voice-edit/projects")
                                .join(id)
                                .join("project.json"),
                        )?;
                        let head = project["head"]
                            .as_u64()
                            .ok_or("Project revision unavailable")?
                            as usize;
                        if project["versions"][head]["audio"].as_str() == Some(file.as_str()) {
                            Some(Input::Editing {
                                id: id.clone(),
                                original: false,
                            })
                        } else if let Some(index) =
                            project["versions"].as_array().and_then(|versions| {
                                versions
                                    .iter()
                                    .rposition(|v| v["audio"].as_str() == Some(file.as_str()))
                            })
                        {
                            let (name, bytes, context) =
                                resolve_editing(root, id.clone(), false, false, Some(index))?;
                            valid_name(&name)?;
                            validate_context(&context, &bytes)?;
                            return Ok(Package {
                                name,
                                bytes,
                                context,
                            });
                        } else {
                            None
                        }
                    }
                }
                _ => None,
            };
            if let Some(input) = input {
                return resolve(root, input);
            }
        }
    }
    let (name, bytes) = crate::audio_import::prepare(name, read_bounded(&path, AUDIO_LIMIT)?)?;
    crate::asr::validate_audio(&bytes)?;
    let context = base(
        &bytes,
        Reference {
            kind: "file".into(),
            id: hash(&bytes),
        },
    );
    Ok(Package {
        name,
        bytes,
        context,
    })
}
#[tauri::command]
pub async fn resolve_recording_transfer(
    app: tauri::AppHandle,
    input: Input,
) -> Result<tauri::ipc::Response, String> {
    let root = app
        .path()
        .app_data_dir()
        .map_err(|_| "Studio storage unavailable")?;
    tauri::async_runtime::spawn_blocking(move || {
        resolve(&root, input)
            .and_then(encode_package)
            .map(tauri::ipc::Response::new)
    })
    .await
    .map_err(|_| "Opening the recording was interrupted".to_string())?
}
fn encode_package(package: Package) -> Result<Vec<u8>, String> {
    let metadata = serde_json::to_vec(&json!({"name":package.name,"context":package.context}))
        .map_err(|_| "Recording details could not be transferred")?;
    if metadata.len() > CONTEXT_LIMIT + 4096 {
        return Err("Recording details are too large to transfer.".into());
    }
    let offset = 4 + metadata.len();
    let mut bytes = package.bytes;
    let audio_len = bytes.len();
    bytes
        .try_reserve(offset)
        .map_err(|_| "Not enough memory to open this recording. Try a shorter WAV.")?;
    bytes.resize(audio_len + offset, 0);
    bytes.copy_within(0..audio_len, offset);
    bytes[..4].copy_from_slice(&(metadata.len() as u32).to_le_bytes());
    bytes[4..offset].copy_from_slice(&metadata);
    Ok(bytes)
}
#[tauri::command]
pub async fn adopt_recording_transfer(
    app: tauri::AppHandle,
    audio_id: String,
) -> Result<Option<transcripts::Transcript>, String> {
    let root = app
        .path()
        .app_data_dir()
        .map_err(|_| "Studio storage unavailable")?;
    tauri::async_runtime::spawn_blocking(move || adopt(&root, &audio_id))
        .await
        .map_err(|_| "Saving the transferred transcript was interrupted".to_string())?
}
pub fn adopt(root: &Path, audio_id: &str) -> Result<Option<transcripts::Transcript>, String> {
    valid_id(audio_id)?;
    let package = resolve_draft(root, audio_id.into(), true)?;
    let context = package.context;
    if context.text.trim().is_empty() && context.transcript.is_none() {
        return Ok(None);
    }
    let rel = PathBuf::from("recordings")
        .join(audio_id)
        .join(&package.name);
    let audio = managed(root, &rel)?;
    let snapshot = context.transcript.as_ref().filter(|t| {
        context.transcript_audio_sha256.as_deref() == Some(context.audio_sha256.as_str())
            && t.text == context.text
    });
    let model = if let Some(snapshot) = snapshot {
        transcripts::ModelTranscript {
            processing: snapshot.processing.clone(),
            model: snapshot.model.clone(),
            text: snapshot.original_text.clone(),
            words: snapshot
                .words
                .iter()
                .map(|w| transcripts::ModelWord {
                    word: w.word.clone(),
                    start: w.start,
                    end: w.end,
                })
                .collect(),
            model_version: snapshot.model_version.clone(),
        }
    } else {
        transcripts::ModelTranscript {
            processing: None,
            model: Some("studio-transfer".into()),
            model_version: "imported".into(),
            text: context.text.clone(),
            words: if context.timings_current {
                context
                    .words
                    .iter()
                    .map(|w| transcripts::ModelWord {
                        word: w.word.clone(),
                        start: w.start,
                        end: w.end,
                    })
                    .collect()
            } else {
                vec![]
            },
        }
    };
    let store = transcripts::TranscriptStore::new(root.join("transcripts"))
        .map_err(|_| "Transcript storage unavailable")?;
    store
        .create_transferred(
            audio,
            &model,
            Some(audio_id),
            &serde_json::to_vec(&context).map_err(|_| "Recording details unavailable")?,
            snapshot,
        )
        .map(Some)
        .map_err(|_| {
            "Could not save the transferred transcript. Your imported recording is still available."
                .into()
        })
}
#[tauri::command]
pub async fn get_recording_transfer_path(
    app: tauri::AppHandle,
    input: Input,
) -> Result<String, String> {
    let root = app
        .path()
        .app_data_dir()
        .map_err(|_| "Studio storage unavailable")?;
    tauri::async_runtime::spawn_blocking(move || {
        let package = resolve(&root, input)?;
        let source = &package.context.source;
        let id = &source.id;
        let rel = match source.kind.as_str() {
            "transcript" => PathBuf::from("transcripts").join(id).join("audio.wav"),
            "recording" => PathBuf::from("recordings").join(id).join(&package.name),
            "cleanup-recording" => PathBuf::from("cleanup-recordings")
                .join(id)
                .join("audio.wav"),
            "cleanup" | "cleanup-original" => {
                PathBuf::from("cleanup-results")
                    .join(id)
                    .join(if source.kind == "cleanup" {
                        "cleaned.wav"
                    } else {
                        "original.wav"
                    })
            }
            "editing" | "editing-original" | "synthesis" => {
                let folder = PathBuf::from("voice-edit/projects").join(id);
                let file = if source.kind == "synthesis" {
                    "synthesis.wav".to_owned()
                } else if source.kind == "editing-original" {
                    "original.wav".to_owned()
                } else {
                    let p = read_json(&root, &folder.join("project.json"))?;
                    p["versions"]
                        [p["head"].as_u64().ok_or("Project revision unavailable")? as usize]
                        ["audio"]
                        .as_str()
                        .ok_or("Project audio unavailable")?
                        .to_owned()
                };
                valid_name(&file)?;
                folder.join(file)
            }
            _ => {
                return Err("Save this recording in a project before copying its WAV path.".into())
            }
        };
        Ok(managed(&root, &rel)?.to_string_lossy().into_owned())
    })
    .await
    .map_err(|_| "Reading the recording path was interrupted".to_string())?
}
#[tauri::command]
pub async fn copy_recording_transfer_path(
    app: tauri::AppHandle,
    input: Input,
) -> Result<(), String> {
    let path = get_recording_transfer_path(app, input).await?;
    crate::transcript_actions::copy(&path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};
    static FIXTURE_SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let p = std::env::temp_dir().join(format!(
                "studio-transfer-{}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                FIXTURE_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            fs::create_dir(&p).unwrap();
            Self(p)
        }
        fn wav(&self, name: &str, value: i16) -> PathBuf {
            let p = self.0.join(name);
            if let Some(parent) = p.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            let mut writer = hound::WavWriter::create(
                &p,
                hound::WavSpec {
                    channels: 1,
                    sample_rate: 16000,
                    bits_per_sample: 16,
                    sample_format: hound::SampleFormat::Int,
                },
            )
            .unwrap();
            for _ in 0..8000 {
                writer.write_sample(value).unwrap();
            }
            writer.finalize().unwrap();
            p
        }
        fn transcript(&self) -> transcripts::Transcript {
            let audio = self.wav("Original recording.wav", 17);
            let store = transcripts::TranscriptStore::new(self.0.join("transcripts")).unwrap();
            let t = store
                .create(
                    &audio,
                    &transcripts::ModelTranscript {
                        processing: None,
                        model: Some("amharic-ctc".into()),
                        model_version: "v0.2".into(),
                        text: "ሰላም".into(),
                        words: vec![transcripts::ModelWord {
                            word: "ሰላም".into(),
                            start: 0.1,
                            end: 0.3,
                        }],
                    },
                )
                .unwrap();
            store.correct(&t.id, t.revision, 0, 1, "ጤና".into()).unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn copied_wav_paths_recover_checked_text_timings_and_model_provenance() {
        let f = Fixture::new();
        let t = f.transcript();
        let path = f.0.join("transcripts").join(&t.id).join("audio.wav");
        let package = resolve(
            &f.0,
            Input::Path {
                path: format!("\"{}\"", path.display()),
            },
        )
        .unwrap();
        assert_eq!(package.context.text, "ጤና");
        assert_eq!(package.context.words[0].start, 0.1);
        assert_eq!(package.context.model_version.as_deref(), Some("v0.2"));
        assert_eq!(
            serde_json::to_value(package.context.transcript.unwrap().corrections).unwrap(),
            serde_json::to_value(t.corrections).unwrap()
        );
        assert_eq!(package.bytes, fs::read(path).unwrap());
        assert!(package.context.timings_current);
        let url = tauri::Url::from_file_path(f.0.join("Original recording.wav")).unwrap();
        let external = resolve(
            &f.0,
            Input::Path {
                path: url.to_string(),
            },
        )
        .unwrap();
        assert_eq!(external.context.source.kind, "file");
        assert!(external.context.text.is_empty());
    }
    #[test]
    fn independent_history_copy_preserves_corrections_and_survives_source_deletion() {
        let f = Fixture::new();
        let original = f.transcript();
        let source = resolve(
            &f.0,
            Input::Transcript {
                id: original.id.clone(),
            },
        )
        .unwrap();
        let bytes = source.bytes.clone();
        let imported =
            crate::asr::import_audio(&f.0, source.name, source.bytes, Some(source.context))
                .unwrap();
        let copied = adopt(&f.0, &imported.id).unwrap().unwrap();
        assert_ne!(copied.id, original.id);
        assert_eq!(copied.model_version, original.model_version);
        assert_eq!(copied.original_text, original.original_text);
        assert_eq!(
            serde_json::to_value(&copied.corrections).unwrap(),
            serde_json::to_value(&original.corrections).unwrap()
        );
        let store = transcripts::TranscriptStore::new(f.0.join("transcripts")).unwrap();
        store.delete(&original.id, original.revision).unwrap();
        let restored = resolve(&f.0, Input::Transcript { id: copied.id }).unwrap();
        assert_eq!(restored.bytes, bytes);
        assert_eq!(restored.context.text, "ጤና");
        assert!(!restored.context.transcript_history.is_empty());
        assert_eq!(fs::read(f.0.join("Original recording.wav")).unwrap(), bytes);
    }
    #[test]
    fn cleanup_keeps_text_and_audit_but_marks_carried_word_timings_as_reference() {
        let f = Fixture::new();
        let t = f.transcript();
        let package = resolve(&f.0, Input::Transcript { id: t.id }).unwrap();
        let folder = f.0.join("cleanup-results/clean-1");
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("original.wav"), &package.bytes).unwrap();
        f.wav("cleanup-results/clean-1/cleaned.wav", 29);
        write_context(&folder, Some(&package.context), &package.bytes).unwrap();
        fs::write(
            folder.join("result.json"),
            serde_json::to_vec(
                &json!({"id":"clean-1","name":"Cleaned.wav","model_version":"fixture-cleanup"}),
            )
            .unwrap(),
        )
        .unwrap();
        let cleaned = resolve(
            &f.0,
            Input::Cleanup {
                id: "clean-1".into(),
                original: false,
            },
        )
        .unwrap();
        assert_eq!(cleaned.context.text, "ጤና");
        assert_eq!(cleaned.context.words.len(), 1);
        assert!(!cleaned.context.timings_current);
        assert!(cleaned.context.transcript.is_some());
        assert!(cleaned.context.cleanup.is_some());
        assert_ne!(cleaned.context.audio_sha256, package.context.audio_sha256);
        let imported =
            crate::asr::import_audio(&f.0, cleaned.name, cleaned.bytes, Some(cleaned.context))
                .unwrap();
        let copied = adopt(&f.0, &imported.id).unwrap().unwrap();
        assert_eq!(copied.text, "ጤና");
        assert!(copied.words.is_empty());
        assert_eq!(copied.model_version, "imported");
    }
    #[test]
    fn edited_and_generated_audio_keep_synthetic_marks_and_exact_selected_audio() {
        let f = Fixture::new();
        f.wav("voice-edit/projects/edit-1/original.wav", 1);
        f.wav("voice-edit/projects/edit-1/revision-1.wav", 2);
        f.wav("voice-edit/projects/edit-1/preview.wav", 3);
        f.wav("voice-edit/projects/edit-1/synthesis.wav", 4);
        let dir = f.0.join("voice-edit/projects/edit-1");
        let region = json!({"span_in":[0.1,0.3],"span_out":[0.1,0.3]});
        fs::write(dir.join("project.json"),serde_json::to_vec(&json!({"id":"edit-1","name":"Edited.wav","head":1,"versions":[{"audio":"original.wav","text":"ሰላም","words":[],"regions":[]},{"audio":"revision-1.wav","text":"ጤና","words":[{"word":"ጤና","start":0.1,"end":0.3}],"regions":[region]}]})).unwrap()).unwrap();
        fs::write(
            dir.join("synthesis-context.json"),
            br#"{"text":"Generated script","kind":"synthesis"}"#,
        )
        .unwrap();
        let current = resolve(
            &f.0,
            Input::Editing {
                id: "edit-1".into(),
                original: false,
            },
        )
        .unwrap();
        assert_eq!(current.context.text, "ጤና");
        assert_eq!(current.context.edited_regions.len(), 1);
        assert_eq!(current.bytes, fs::read(dir.join("revision-1.wav")).unwrap());
        let preview = resolve(
            &f.0,
            Input::Path {
                path: dir.join("preview.wav").to_string_lossy().into(),
            },
        )
        .unwrap();
        assert_eq!(preview.bytes, fs::read(dir.join("preview.wav")).unwrap());
        assert_eq!(preview.context.source.kind, "file");
        let generated = resolve(
            &f.0,
            Input::Synthesis {
                id: "edit-1".into(),
            },
        )
        .unwrap();
        assert_eq!(generated.context.text, "Generated script");
        assert_eq!(generated.context.edited_regions.len(), 1);
        assert!(generated.context.words.is_empty());
    }
    #[test]
    fn hash_timing_and_reference_validation_prevent_wrong_audio_metadata_pairings() {
        let f = Fixture::new();
        let t = f.transcript();
        let mut package = resolve(&f.0, Input::Transcript { id: t.id }).unwrap();
        package.context.audio_sha256 = "00".repeat(32);
        assert!(crate::asr::import_audio(
            &f.0,
            package.name,
            package.bytes.clone(),
            Some(package.context.clone())
        )
        .is_err());
        assert_eq!(fs::read_dir(f.0.join("recordings")).unwrap().count(), 0);
        package.context.audio_sha256 = hash(&package.bytes);
        package.context.words[0].end = 10.;
        assert!(validate_context(&package.context, &package.bytes).is_err());
        assert!(resolve(
            &f.0,
            Input::Transcript {
                id: "../secret".into()
            }
        )
        .is_err());
        assert!(serde_json::from_value::<Input>(
            json!({"kind":"transcript","id":"test","path":"/private/file.wav"})
        )
        .is_err());
        for path in [
            "relative.wav",
            "https://server.invalid/file.wav",
            "file://server.invalid/file.wav",
        ] {
            assert!(resolve(&f.0, Input::Path { path: path.into() }).is_err());
        }
    }
    #[test]
    fn draft_kind_never_resolves_a_different_tool_with_the_same_identifier() {
        let f = Fixture::new();
        f.wav("recordings/same/asr.wav", 1);
        f.wav("cleanup-recordings/same/audio.wav", 2);
        fs::write(f.0.join("recordings/same/name.json"), br#""asr.wav""#).unwrap();
        fs::write(
            f.0.join("cleanup-recordings/same/name.json"),
            br#""cleanup.wav""#,
        )
        .unwrap();
        let asr = resolve(&f.0, Input::Recording { id: "same".into() }).unwrap();
        let cleanup = resolve(&f.0, Input::CleanupRecording { id: "same".into() }).unwrap();
        assert_ne!(asr.bytes, cleanup.bytes);
        assert_eq!(cleanup.name, "cleanup.wav");
    }
    #[test]
    fn binary_handoff_keeps_pcm_identical_without_json_byte_array_expansion() {
        let f = Fixture::new();
        let t = f.transcript();
        let package = resolve(&f.0, Input::Transcript { id: t.id }).unwrap();
        let audio = package.bytes.clone();
        let wire = encode_package(package).unwrap();
        let count = u32::from_le_bytes(wire[..4].try_into().unwrap()) as usize;
        let metadata: Value = serde_json::from_slice(&wire[4..4 + count]).unwrap();
        assert_eq!(metadata["context"]["text"], "ጤና");
        assert!(metadata.get("bytes").is_none());
        assert_eq!(&wire[4 + count..], audio.as_slice());
    }
    #[test]
    fn intentionally_empty_checked_transcripts_are_not_downgraded_to_unknown_drafts() {
        let f = Fixture::new();
        let t = f.transcript();
        let store = transcripts::TranscriptStore::new(f.0.join("transcripts")).unwrap();
        let t = store
            .correct(&t.id, t.revision, 0, 1, String::new())
            .unwrap();
        let package = resolve(&f.0, Input::Transcript { id: t.id }).unwrap();
        let item =
            crate::asr::import_audio(&f.0, package.name, package.bytes, Some(package.context))
                .unwrap();
        let copied = adopt(&f.0, &item.id).unwrap().unwrap();
        assert!(copied.text.is_empty());
        assert_eq!(copied.corrections.len(), 2);
    }
    #[test]
    fn paths_resolve_latest_metadata_for_identical_audio_and_historical_audio_revisions() {
        let f = Fixture::new();
        f.wav("voice-edit/projects/history/original.wav", 1);
        f.wav("voice-edit/projects/history/edited.wav", 2);
        let dir = f.0.join("voice-edit/projects/history");
        let project = json!({"id":"history","name":"History.wav","head":1,"versions":[{"audio":"original.wav","text":"original","words":[],"regions":[]},{"audio":"original.wav","text":"corrected transcript","words":[],"regions":[]},{"audio":"edited.wav","text":"edited speech","words":[],"regions":[{"span_out":[0.1,0.3],"span_in":[0.1,0.3]}]}]});
        fs::write(
            dir.join("project.json"),
            serde_json::to_vec(&project).unwrap(),
        )
        .unwrap();
        let same = resolve(
            &f.0,
            Input::Path {
                path: dir.join("original.wav").to_string_lossy().into(),
            },
        )
        .unwrap();
        assert_eq!(same.context.text, "corrected transcript");
        let historical = resolve(
            &f.0,
            Input::Path {
                path: dir.join("edited.wav").to_string_lossy().into(),
            },
        )
        .unwrap();
        assert_eq!(historical.context.text, "edited speech");
        assert_eq!(historical.context.edited_regions.len(), 1);
        assert_eq!(historical.context.edit_log.unwrap()["head"], 2);
    }
    #[cfg(unix)]
    #[test]
    fn symlinks_and_non_audio_files_cannot_be_used_as_recording_transfers() {
        use std::os::unix::fs::symlink;
        let f = Fixture::new();
        let t = f.transcript();
        let audio = f.0.join("transcripts").join(&t.id).join("audio.wav");
        let external = f.wav("External.wav", 9);
        fs::remove_file(&audio).unwrap();
        symlink(&external, &audio).unwrap();
        assert!(resolve(&f.0, Input::Transcript { id: t.id }).is_err());
        let link = f.0.join("Alias.wav");
        symlink(&external, &link).unwrap();
        assert!(resolve(
            &f.0,
            Input::Path {
                path: link.to_string_lossy().into()
            }
        )
        .is_err());
        fs::write(f.0.join("Fake.wav"), b"not audio").unwrap();
        assert!(resolve(
            &f.0,
            Input::Path {
                path: f.0.join("Fake.wav").to_string_lossy().into()
            }
        )
        .is_err());
    }
}
