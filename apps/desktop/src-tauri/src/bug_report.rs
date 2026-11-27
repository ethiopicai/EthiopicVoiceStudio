//! Project a small diagnostics JSON object into the reviewed web form. No user content.
use std::process::Stdio;
const FORM: &str = "https://ethiopic.ai/studio/bugreport";
use serde_json::{json, Value};
use tauri::Manager;
fn token(value: &str) -> String {
    if value.len() <= 80
        && !value.is_empty()
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-._".contains(&c))
    {
        value.into()
    } else {
        String::new()
    }
}
fn version(value: &str) -> String {
    if value.len() <= 96
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b" ._()+-".contains(&c))
    {
        value.into()
    } else {
        String::new()
    }
}
fn tool(
    task: &str,
    id: &str,
    model_version: &str,
    backend: &str,
    device: &str,
    state: &str,
) -> Value {
    json!({"tool":task,"model_id":token(id),"model_version":version(model_version),"backend":token(backend),"device":if ["cpu","cuda","cuda:0","mps"].contains(&device){device}else{""},"state":state})
}
fn metadata(root: &std::path::Path, relative: &str) -> Option<Value> {
    let path = root.join(relative);
    let meta = std::fs::symlink_metadata(&path).ok()?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 65536 {
        return None;
    }
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}
fn collect(app: &tauri::AppHandle) -> Value {
    let root = app.path().app_data_dir().ok();
    let mut tools = vec![];
    let mut revision = String::new();
    if let Some(root) = root {
        if let Ok(Some(bundle)) = crate::recognition_bundle::read(&root.join("asr/active/bundle")) {
            for (task, p) in [
                ("transcription", bundle.transcription),
                ("dictation", bundle.dictation),
            ] {
                tools.push(tool(
                    task,
                    &p.variant,
                    &p.model_version,
                    &p.backend,
                    &p.device,
                    "installed",
                ));
            }
        } else if let Some(value) = metadata(&root, "asr/active/bundle/model/version.json") {
            let release = metadata(&root, "asr/active/bundle/release.json");
            for task in ["transcription", "dictation"] {
                tools.push(tool(
                    task,
                    release
                        .as_ref()
                        .and_then(|v| v["variant"].as_str())
                        .unwrap_or("legacy-amharic-ctc"),
                    release
                        .as_ref()
                        .and_then(|v| v["model_version"].as_str())
                        .or_else(|| value["model_version"].as_str())
                        .unwrap_or_default(),
                    "torch",
                    "",
                    "installed",
                ));
            }
        } else {
            for task in ["transcription", "dictation"] {
                tools.push(tool(task, "", "", "", "", "not_installed"));
            }
        }
        if let Ok(manifest) = crate::voice_editing::setup_manifest(&root) {
            revision = manifest.hf_revision.clone().unwrap_or_default();
        }
        let speech = app
            .try_state::<crate::voice_editing::EditingState>()
            .and_then(|state| crate::voice_editing::generation_status(&state).ok())
            .and_then(|v| serde_json::to_value(v).ok());
        if let Some(value) = speech {
            let provider = value["provider"].as_str().unwrap_or_default();
            tools.push(tool(
                "text-to-speech",
                provider,
                value["modelVersion"].as_str().unwrap_or_default(),
                if provider == "tts-flow_lm-90m-amh-fp32" {
                    "small-cpu"
                } else {
                    "omnivoice"
                },
                "",
                if value["loaded"] == true {
                    "loaded"
                } else if value["ready"] == true {
                    "ready"
                } else {
                    "not_installed"
                },
            ));
        } else {
            tools.push(tool("text-to-speech", "", "", "", "", "unknown"));
        }
        if let Some(value) = metadata(&root, "studio-runtime/active/installation.json") {
            if value["capabilities"]
                .as_array()
                .is_some_and(|c| c.iter().any(|v| v == "edit-codec_llm-0.6b-amh"))
            {
                tools.push(tool(
                    "voice-editing",
                    "edit-codec_llm-0.6b-amh",
                    "",
                    "omnivoice",
                    "",
                    "installed",
                ));
            }
        }
        if let Some(value) = metadata(&root, "cleanup/active/bundle/version.json") {
            tools.push(tool(
                "voice-cleanup",
                "cleanup-reuse",
                value["model_version"].as_str().unwrap_or_default(),
                "reuse",
                "",
                "installed",
            ));
        }
        if let Some(value) = metadata(&root, "noise-reduction/active/installation.json") {
            tools.push(tool(
                "noise-reduction",
                "inworld-voice-isolator-v1-realtime",
                value["release"].as_str().unwrap_or_default(),
                "torch",
                value["device"].as_str().unwrap_or_default(),
                "installed",
            ));
        }
    }
    diagnostics(
        &app.package_info().version.to_string(),
        std::env::consts::OS,
        sysinfo::System::os_version().as_deref().unwrap_or_default(),
        std::env::consts::ARCH,
        &revision,
        tools,
    )
}
fn diagnostics(
    app_version: &str,
    os: &str,
    os_version: &str,
    arch: &str,
    revision: &str,
    tools: Vec<Value>,
) -> Value {
    json!({"schema_version":1,"version":app_version,"os":os,"os_version":version(os_version),"arch":arch,"model_catalog_revision":token(revision),"tools":tools})
}
fn form_url(details: &Value) -> Result<String, String> {
    let payload =
        serde_json::to_string(details).map_err(|_| "App details could not be prepared.")?;
    if payload.len() > 6144 {
        return Err("App details are too large. Open the form directly.".into());
    }
    let mut url = tauri::Url::parse(FORM).expect("fixed bug-report URL");
    url.query_pairs_mut().append_pair("diagnostics", &payload);
    let fragment = url.query().unwrap_or_default().to_owned();
    url.set_query(None);
    url.set_fragment(Some(&fragment));
    Ok(url.to_string())
}
#[tauri::command]
pub async fn open_bug_report(app: tauri::AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let url = form_url(&collect(&app))?;
        #[cfg(target_os = "macos")]
        let mut command = {
            let mut c = crate::background_process::command("/usr/bin/open");
            c.arg(url);
            c
        };
        #[cfg(target_os = "windows")]
        let mut command = {
            let mut c = crate::background_process::command("rundll32.exe");
            c.args(["url.dll,FileProtocolHandler", &url]);
            c
        };
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let mut command = {
            let mut c = crate::background_process::command("xdg-open");
            c.arg(url);
            c
        };
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|_| "The bug-report form could not be opened.")?
            .success()
            .then_some(())
            .ok_or_else(|| "The bug-report form could not be opened.".to_owned())
    })
    .await
    .map_err(|_| "The bug-report form could not be opened.".to_owned())?
}
#[tauri::command]
pub async fn get_bug_report(app: tauri::AppHandle) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        serde_json::to_string_pretty(&collect(&app))
            .map_err(|_| "App details could not be prepared.".into())
    })
    .await
    .map_err(|_| "App details could not be prepared.".to_owned())?
}
#[tauri::command]
pub async fn copy_bug_report(app: tauri::AppHandle) -> Result<(), String> {
    let body = get_bug_report(app).await?;
    tauri::async_runtime::spawn_blocking(move || crate::transcript_actions::copy(&body))
        .await
        .map_err(|_| "App details could not be copied.".to_owned())?
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn json_roundtrips_through_fragment_without_a_query() {
        let details = diagnostics(
            "0.1.0-rc.2",
            "windows",
            "10.0.26100",
            "x86_64",
            "a8d70e01",
            vec![tool(
                "dictation",
                "asr-wav2vec_ctc-300m-amh-int8_onnx",
                "v0.3",
                "onnx",
                "cpu",
                "installed",
            )],
        );
        let url = tauri::Url::parse(&form_url(&details).unwrap()).unwrap();
        assert_eq!(url.host_str(), Some("ethiopic.ai"));
        assert_eq!(url.path(), "/studio/bugreport");
        assert!(url.query().is_none());
        let mut query = tauri::Url::parse(FORM).unwrap();
        query.set_query(url.fragment());
        let payload = query
            .query_pairs()
            .find(|(k, _)| k == "diagnostics")
            .unwrap()
            .1
            .into_owned();
        assert_eq!(serde_json::from_str::<Value>(&payload).unwrap(), details);
        assert!(url.as_str().len() < 8192);
    }
    #[test]
    fn local_metadata_is_projected_and_cannot_include_paths_or_report_text() {
        let details = diagnostics(
            "0.1.0-rc.2",
            "macos",
            "15.0",
            "aarch64",
            "abc",
            vec![tool(
                "dictation",
                "C:\\Users\\private",
                "/home/user/private.txt",
                "https://secret.invalid",
                "unknown",
                "installed",
            )],
        );
        let encoded = serde_json::to_string(&details).unwrap();
        assert!(!encoded.contains("private"));
        assert!(!encoded.contains("secret.invalid"));
        assert_eq!(details["tools"][0]["model_id"], "");
        assert!(details.get("installation_id").is_none());
        assert!(details.get("transcript").is_none());
    }
}
