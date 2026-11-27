//! Available host memory for model admission. Keep platform accounting in one place.
//!
//! sysinfo 0.33 subtracts macOS compressor pages from free + inactive pages,
//! even though the compressor occupies a separate pool. This can clamp available
//! memory to zero on a healthy, busy Mac. Read Mach counters directly instead.
//! Counter definitions: https://github.com/apple-oss-distributions/xnu/blob/main/osfmk/mach/vm_statistics.h
//! The conservative free + inactive estimate also matches Deno's macOS host-memory reader.
pub fn available(system: &sysinfo::System) -> u64 {
    #[cfg(target_os = "macos")]
    if let Some(bytes) = macos_available() {
        return bytes.min(system.total_memory());
    }
    // A failed native query stays conservative; never fabricate extra capacity.
    system.available_memory()
}

#[cfg(target_os = "macos")]
fn reclaimable_bytes(free: u32, inactive: u32, page_size: u64) -> u64 {
    // free_count already includes speculative pages. Purgeable pages may overlap
    // active/inactive pools, so do not add them again. Active, wired, compressed
    // pages and swap capacity are not part of this admission estimate.
    u64::from(free)
        .saturating_add(u64::from(inactive))
        .saturating_mul(page_size)
}

#[cfg(target_os = "macos")]
#[allow(deprecated)] // Pinned libc exposes these stable Mach APIs with deprecation annotations.
fn macos_available() -> Option<u64> {
    use std::mem;
    unsafe extern "C" {
        fn mach_port_deallocate(
            task: libc::mach_port_t,
            name: libc::mach_port_t,
        ) -> libc::kern_return_t;
    }
    // SAFETY: sysconf has no pointer arguments. These APIs are available on every
    // supported macOS version. The buffer is sized for the requested Mach count;
    // its zero initialization also supports older kernels returning fewer fields.
    unsafe {
        let page_size = libc::sysconf(libc::_SC_PAGESIZE);
        if page_size <= 0 {
            return None;
        }
        let host = libc::mach_host_self();
        if host == 0 {
            return None;
        }
        let mut stat = mem::zeroed::<libc::vm_statistics64>();
        let mut count = libc::HOST_VM_INFO64_COUNT;
        let result = libc::host_statistics64(
            host,
            libc::HOST_VM_INFO64,
            &mut stat as *mut _ as libc::host_info64_t,
            &mut count,
        );
        // mach_host_self gives us a send right; release it on success and failure.
        mach_port_deallocate(libc::mach_task_self(), host);
        if result != libc::KERN_SUCCESS || count < 3 {
            return None;
        }
        Some(reclaimable_bytes(
            stat.free_count,
            stat.inactive_count,
            page_size as u64,
        ))
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    const GIB: u64 = 1024 * 1024 * 1024;

    #[test]
    fn compressed_pool_cannot_erase_reclaimable_memory() {
        // Reproduce this Mac's failed startup: the old formula reports zero
        // because compressor pages exceed the free/inactive/purgeable sum.
        let free = 18_011_u32;
        let inactive = 842_279_u32;
        let purgeable = 15_486_u32;
        let compressed = 1_055_003_u32;
        let old =
            u64::from(free + inactive + purgeable).saturating_sub(u64::from(compressed)) * 16_384;
        assert_eq!(old, 0);
        assert!(reclaimable_bytes(free, inactive, 16_384) > 5 * GIB);
    }

    #[test]
    fn genuinely_low_memory_still_fails_the_existing_floor() {
        assert!(reclaimable_bytes(1_000, 2_000, 16_384) < 5 * GIB);
        assert_eq!(reclaimable_bytes(0, 0, 16_384), 0);
        assert_eq!(reclaimable_bytes(4, 8, 4_096), 49_152);
        assert_eq!(reclaimable_bytes(u32::MAX, u32::MAX, u64::MAX), u64::MAX);
    }

    #[test]
    #[ignore = "reads live host memory; run explicitly for platform qualification"]
    fn report_host_memory_for_model_admission() {
        let mut system = sysinfo::System::new();
        system.refresh_memory();
        let native = macos_available().expect("Mach memory query must succeed");
        eprintln!(
            "Memory GiB: total={:.2}, sysinfo_available={:.2}, corrected_available={:.2}",
            system.total_memory() as f64 / GIB as f64,
            system.available_memory() as f64 / GIB as f64,
            available(&system) as f64 / GIB as f64,
        );
        assert!(native > 0);
        assert!(system.total_memory() > 0);
        assert!(available(&system) <= system.total_memory());
    }
}
