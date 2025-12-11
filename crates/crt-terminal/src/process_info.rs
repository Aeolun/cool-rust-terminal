// ABOUTME: Platform-specific process information queries.
// ABOUTME: Used to get shell working directory for session restoration.

use std::path::PathBuf;

/// Get the current working directory of a process by PID.
/// Returns None if the process doesn't exist or we can't read its cwd.
#[cfg(target_os = "linux")]
pub fn get_process_cwd(pid: u32) -> Option<PathBuf> {
    std::fs::read_link(format!("/proc/{}/cwd", pid)).ok()
}

#[cfg(target_os = "macos")]
pub fn get_process_cwd(pid: u32) -> Option<PathBuf> {
    use std::ffi::CStr;
    use std::mem::MaybeUninit;

    // Constants from sys/proc_info.h
    const PROC_PIDVNODEPATHINFO: libc::c_int = 9;
    const MAXPATHLEN: usize = 1024;

    #[repr(C)]
    struct VipPath {
        vip_vi: [u8; 152], // vnode_info_path minus the path
        vip_path: [libc::c_char; MAXPATHLEN],
    }

    #[repr(C)]
    struct ProcVnodePathInfo {
        pvi_cdir: VipPath,
        pvi_rdir: VipPath,
    }

    extern "C" {
        fn proc_pidinfo(
            pid: libc::c_int,
            flavor: libc::c_int,
            arg: u64,
            buffer: *mut libc::c_void,
            buffersize: libc::c_int,
        ) -> libc::c_int;
    }

    let mut vpi: MaybeUninit<ProcVnodePathInfo> = MaybeUninit::uninit();
    let buffer_size = std::mem::size_of::<ProcVnodePathInfo>() as libc::c_int;

    let ret = unsafe {
        proc_pidinfo(
            pid as libc::c_int,
            PROC_PIDVNODEPATHINFO,
            0,
            vpi.as_mut_ptr() as *mut libc::c_void,
            buffer_size,
        )
    };

    if ret <= 0 {
        return None;
    }

    let vpi = unsafe { vpi.assume_init() };
    let path_cstr = unsafe { CStr::from_ptr(vpi.pvi_cdir.vip_path.as_ptr()) };

    path_cstr.to_str().ok().map(PathBuf::from)
}

#[cfg(windows)]
pub fn get_process_cwd(_pid: u32) -> Option<PathBuf> {
    // Windows cwd query is complex and we're skipping Windows session restore
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_current_process_cwd() {
        // Get our own PID and verify we can read our cwd
        let pid = std::process::id();
        let cwd = get_process_cwd(pid);

        // Should be able to get our own cwd
        assert!(cwd.is_some(), "Should be able to get current process cwd");

        // Should match std::env::current_dir()
        let expected = std::env::current_dir().unwrap();
        assert_eq!(cwd.unwrap(), expected);
    }

    #[test]
    fn test_nonexistent_process() {
        // PID 0 is typically kernel/init and we shouldn't have access,
        // or use a very high PID that likely doesn't exist
        let cwd = get_process_cwd(99999999);
        assert!(cwd.is_none());
    }
}
