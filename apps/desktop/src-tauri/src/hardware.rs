//! Local hardware inspection shared by setup and inference selection. No uploads or stable device IDs.
use resource_packs::studio::{Hardware, Nvidia};
use std::{
    io::Read,
    process::Stdio,
    time::{Duration, Instant},
};

use crate::background_process::command as background_command;

fn nvidia_inventory() -> Option<String> {
    let mut child = background_command("nvidia-smi")
        .args([
            "--query-gpu=index,compute_cap,memory.total,memory.free",
            "--format=csv,noheader,nounits",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => {
                let mut bytes = Vec::new();
                child
                    .stdout
                    .take()?
                    .take(65536)
                    .read_to_end(&mut bytes)
                    .ok()?;
                return String::from_utf8(bytes).ok();
            }
            Ok(None) if started.elapsed() < Duration::from_secs(2) => {
                std::thread::sleep(Duration::from_millis(20))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn local_hardware_inspection_reports_memory_without_model_resources() {
        let hardware = super::inspect();
        assert_eq!(hardware.os, std::env::consts::OS);
        assert_eq!(hardware.arch, std::env::consts::ARCH);
        assert!(hardware.total_ram_bytes > 0);
    }

    #[cfg(windows)]
    #[test]
    fn background_helpers_have_no_windows_console() {
        let output = super::background_command("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Add-Type -MemberDefinition '[DllImport(\"kernel32.dll\")] public static extern IntPtr GetConsoleWindow();' -Name StudioConsoleProbe -Namespace Studio; [Studio.StudioConsoleProbe]::GetConsoleWindow().ToInt64()",
            ])
            .output()
            .expect("Windows PowerShell must run for the console regression check");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "0");
    }
}
pub fn inspect() -> Hardware {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    sys.refresh_cpu_all();
    let mut gpus = vec![];
    if let Some(out) = nvidia_inventory() {
        for line in out.lines() {
            let p: Vec<_> = line.split(',').map(str::trim).collect();
            if p.len() != 4 {
                continue;
            }
            let cc: Vec<_> = p[1].split('.').collect();
            if cc.len() != 2 {
                continue;
            }
            if let (Ok(index), Ok(major), Ok(minor), Ok(total), Ok(free)) = (
                p[0].parse(),
                cc[0].parse(),
                cc[1].parse(),
                p[2].parse::<u64>(),
                p[3].parse::<u64>(),
            ) {
                gpus.push(Nvidia {
                    index,
                    compute_major: major,
                    compute_minor: minor,
                    total_vram_bytes: total * 1024 * 1024,
                    free_vram_bytes: free * 1024 * 1024,
                });
            }
        }
    }
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    let avx2 = Some(std::is_x86_feature_detected!("avx2"));
    #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
    let avx2 = None;
    Hardware {
        os: std::env::consts::OS.into(),
        arch: std::env::consts::ARCH.into(),
        total_ram_bytes: sys.total_memory(),
        free_ram_bytes: crate::memory::available(&sys),
        physical_cores: sys.physical_core_count().map(|v| v as u32),
        avx2,
        nvidia: gpus,
    }
}
