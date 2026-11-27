use resource_packs::studio::{Hardware, Manifest, Preference, Task};
fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("Pass the Studio manifest path");
    let manifest = Manifest::parse(&std::fs::read_to_string(path).expect("Read the manifest"))
        .expect("Validate the model handoff");
    println!(
        "Validated {} variants. Published hosting: {}.",
        manifest.variants.len(),
        manifest.hf_repo.is_some()
    );
    let profile = Hardware {
        os: "macos".into(),
        arch: "aarch64".into(),
        total_ram_bytes: 8 * 1024 * 1024 * 1024,
        free_ram_bytes: 4 * 1024 * 1024 * 1024,
        physical_cores: Some(4),
        avx2: None,
        nvidia: vec![],
    };
    println!("Example Apple Silicon profile (8 GiB total, 4 GiB available); selection only, no inference benchmark:");
    for task in [
        Task::Transcription,
        Task::Dictation,
        Task::Corrector,
        Task::Tts { opt_in: false },
    ] {
        println!(
            "{}",
            serde_json::to_string(
                &manifest
                    .select(&profile, task, Preference::Automatic)
                    .expect("Select a compatible profile")
            )
            .unwrap()
        );
    }
}
