#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_updates;
mod asr;
mod asr_runtime;
mod audio_import;
mod background_process;
mod bug_report;
mod cleanup;
mod dictation;
mod dictation_shortcut;
mod download_setup;
mod fn_shortcut;
mod generation_controls;
mod hardware;
mod hardware_setup;
mod memory;
mod microphone_permission;
mod model_health;
mod naming_migration;
mod noise_reduction;
mod recognition_bundle;
mod recording_titles;
mod recording_transfer;
mod resource_setup;
mod resource_verification;
mod speech_text;
mod speech_versions;
mod studio_manifest;
mod transcript_actions;
mod voice_editing;
mod voice_history;
mod voice_setup;

use serde::Serialize;
use tauri::{Manager, State};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SystemSummary {
    os: &'static str,
    arch: &'static str,
    logical_cpus: usize,
    total_memory_bytes: u64,
}

#[tauri::command]
fn system_summary() -> SystemSummary {
    let mut system = sysinfo::System::new();
    system.refresh_memory();
    SystemSummary {
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        logical_cpus: std::thread::available_parallelism()
            .map(usize::from)
            .unwrap_or(1),
        total_memory_bytes: system.total_memory(),
    }
}

#[tauri::command]
async fn get_home_page(
    state: State<'_, home_content::HomeService>,
) -> Result<home_content::HomePage, String> {
    Ok(state.page())
}

#[tauri::command]
async fn refresh_home_page(app: tauri::AppHandle) -> Result<home_content::HomePage, String> {
    tauri::async_runtime::spawn_blocking(move || app.state::<home_content::HomeService>().refresh())
        .await
        .map_err(|_| "Home content could not refresh".into())
}

fn main() {
    tauri::Builder::default()
        .on_permission_request(|webview, kind| {
            if cfg!(target_os = "macos") {
                microphone_permission::response(webview.label(), webview.url().ok().as_ref(), kind)
            } else {
                tauri::webview::PermissionResponse::Default
            }
        })
        .setup(|app| {
            let root = app.path().app_data_dir()?;
            // A broken resource install must not prevent access to saved transcripts.
            let _ = resource_setup::recover_on_start(&root);
            app.manage(resource_setup::SetupState::new(root));
            app.manage(voice_setup::VoiceSetupState::new(
                app.path().app_data_dir()?,
            ));
            app.manage(studio_manifest::Registry::new());
            app.manage(model_health::ModelHealth::new(app.path().app_data_dir()?));
            app.manage(app_updates::UpdateService::new(app.path().app_data_dir()?));
            app.manage(voice_editing::EditingState::new(app.path().app_data_dir()?));
            app.manage(asr::AsrState::new(app.path().app_data_dir()?));
            app.manage(cleanup::CleanupState::new(app.path().app_data_dir()?));
            app.manage(dictation::DictationState::new(app.path().app_data_dir()?));
            app.manage(fn_shortcut::FnState::new(app.path().app_data_dir()?));
            fn_shortcut::initialize(app.handle());
            app.manage(home_content::HomeService::new(
                app.path().app_data_dir()?.join("home"),
            ));
            voice_setup::attach(app.handle());
            model_health::initialize(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            model_health::get_model_health,
            model_health::check_model_health,
            app_updates::get_app_update_status,
            app_updates::check_app_update,
            app_updates::set_app_update_preferences,
            app_updates::dismiss_app_update,
            app_updates::open_app_update,
            download_setup::get_asset_download_plan,
            download_setup::get_setup_introduction,
            download_setup::acknowledge_setup_introduction,
            system_summary,
            voice_history::delete_voice_history,
            voice_history::list_voice_generations,
            speech_versions::get_speech_versions,
            voice_editing::synthesize_speech,
            bug_report::get_bug_report,
            bug_report::open_bug_report,
            bug_report::copy_bug_report,
            recording_transfer::resolve_recording_transfer,
            recording_transfer::adopt_recording_transfer,
            recording_titles::list_recording_titles,
            recording_titles::rename_recording,
            recording_transfer::get_recording_transfer_path,
            recording_transfer::copy_recording_transfer_path,
            fn_shortcut::get_fn_shortcut_status,
            fn_shortcut::set_fn_shortcut_enabled,
            fn_shortcut::set_dictation_shortcut,
            fn_shortcut::set_dictation_shortcut_capture,
            fn_shortcut::bind_fn_dictation,
            fn_shortcut::update_fn_overlay,
            fn_shortcut::finish_fn_dictation,
            studio_manifest::get_studio_model_source,
            studio_manifest::get_studio_release_catalog,
            voice_editing::get_voice_generation_activity,
            voice_editing::get_voice_generation_status,
            voice_editing::prepare_voice_generation,
            voice_editing::get_default_voice,
            voice_editing::get_voice_edit_status,
            voice_editing::get_voice_edit_busy,
            voice_editing::accept_voice_edit_consent,
            voice_editing::set_voice_edit_opt_in,
            voice_editing::prepare_voice_edit,
            voice_editing::cancel_voice_edit,
            voice_editing::voice_edit_action,
            voice_editing::list_voice_edit_projects,
            voice_editing::import_voice_edit_project,
            voice_editing::get_voice_edit_audio,
            voice_editing::export_voice_edit_project,
            transcript_actions::copy_transcript_text,
            transcript_actions::copy_transcript_wav_path,
            transcript_actions::get_transcript_wav_path,
            transcript_actions::export_transcript,
            dictation::start_dictation,
            dictation::feed_dictation,
            dictation::stop_dictation,
            dictation::cancel_dictation,
            dictation::get_dictation_session,
            dictation::get_active_dictation_session,
            get_home_page,
            cleanup::get_cleanup_status,
            cleanup::import_cleanup_audio,
            cleanup::start_cleanup,
            cleanup::get_cleanup_job,
            cleanup::get_active_cleanup_job,
            cleanup::cancel_cleanup,
            cleanup::list_cleanup_results,
            cleanup::get_cleanup_audio,
            cleanup::list_cleanup_recordings,
            cleanup::get_cleanup_recording_audio,
            cleanup::export_cleanup_audio,
            refresh_home_page,
            hardware_setup::get_setup_plan,
            resource_setup::get_resource_setup,
            voice_setup::get_voice_setup,
            voice_setup::start_voice_setup,
            voice_setup::pause_voice_setup,
            resource_setup::start_resource_setup,
            resource_setup::pause_resource_setup,
            asr::get_asr_status,
            asr_runtime::get_asr_runtime_status,
            asr_runtime::set_asr_keep_ready,
            asr_runtime::unload_asr_model,
            noise_reduction::get_noise_reduction_status,
            noise_reduction::set_noise_reduction_enabled,
            noise_reduction::start_noise_reduction_setup,
            noise_reduction::pause_noise_reduction_setup,
            asr::import_transcription_audio,
            asr::start_transcription,
            asr::get_transcription_job,
            asr::get_active_transcription_job,
            asr::cancel_transcription,
            asr::list_transcripts,
            asr::delete_transcript,
            asr::delete_transcripts,
            asr::delete_transcription_recording,
            asr::list_transcription_recordings,
            asr::get_transcription_recording_audio,
            asr::get_transcript,
            asr::get_transcript_audio,
            asr::remove_transcript_word,
            asr::correct_transcript_word
        ])
        .build(tauri::generate_context!())
        .expect("desktop application failed")
        .run(|app, event| {
            if matches!(event, tauri::RunEvent::ExitRequested { .. }) {
                app.state::<model_health::ModelHealth>().stop();
                app.state::<voice_editing::EditingState>().shutdown();
                app.state::<fn_shortcut::FnState>().shutdown();
                app.state::<resource_setup::SetupState>().pause();
                app.state::<voice_setup::VoiceSetupState>().pause();
                asr::cancel_all(app);
                cleanup::cancel_all(app);
                dictation::cancel_all(app);
                asr_runtime::shutdown();
            }
        });
}
