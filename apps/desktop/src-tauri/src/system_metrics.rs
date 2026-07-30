use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::State;

#[derive(Clone, Copy, Debug)]
struct CpuTimes {
    idle: u64,
    total: u64,
}

#[derive(Default)]
pub(crate) struct SystemMetricsMonitor {
    previous_cpu: Mutex<Option<CpuTimes>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SystemMetricsSnapshot {
    sampled_at: u64,
    cpu: CpuMetric,
    memory: StorageMetric,
    disk: DiskSummary,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CpuMetric {
    usage_percent: f64,
    logical_cores: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StorageMetric {
    used_bytes: u64,
    total_bytes: u64,
    usage_percent: f64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DiskSummary {
    used_bytes: u64,
    total_bytes: u64,
    usage_percent: f64,
    volumes: Vec<DiskVolume>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DiskVolume {
    name: String,
    mount_point: String,
    used_bytes: u64,
    total_bytes: u64,
    usage_percent: f64,
}

#[tauri::command]
pub(crate) fn get_system_metrics(
    monitor: State<'_, SystemMetricsMonitor>,
) -> Result<SystemMetricsSnapshot, String> {
    collect_system_metrics(&monitor)
}

fn collect_system_metrics(monitor: &SystemMetricsMonitor) -> Result<SystemMetricsSnapshot, String> {
    let current_cpu = platform::read_cpu_times()?;
    let usage_percent = {
        let mut previous = monitor
            .previous_cpu
            .lock()
            .map_err(|_| "CPU 监控状态不可用".to_owned())?;
        let usage = previous
            .map(|sample| cpu_usage_percent(sample, current_cpu))
            .unwrap_or(0.0);
        *previous = Some(current_cpu);
        usage
    };

    let (memory_used, memory_total) = platform::read_memory()?;
    let volumes = platform::read_disks()?;
    let disk_total = volumes.iter().fold(0_u64, |total, volume| {
        total.saturating_add(volume.total_bytes)
    });
    let disk_used = volumes.iter().fold(0_u64, |total, volume| {
        total.saturating_add(volume.used_bytes)
    });

    Ok(SystemMetricsSnapshot {
        sampled_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_millis() as u64),
        cpu: CpuMetric {
            usage_percent,
            logical_cores: std::thread::available_parallelism()
                .map(usize::from)
                .unwrap_or(1),
        },
        memory: storage_metric(memory_used, memory_total),
        disk: DiskSummary {
            used_bytes: disk_used,
            total_bytes: disk_total,
            usage_percent: percentage(disk_used, disk_total),
            volumes,
        },
    })
}

fn cpu_usage_percent(previous: CpuTimes, current: CpuTimes) -> f64 {
    let elapsed = current.total.saturating_sub(previous.total);
    let idle = current.idle.saturating_sub(previous.idle);
    if elapsed == 0 {
        return 0.0;
    }
    ((elapsed.saturating_sub(idle)) as f64 * 100.0 / elapsed as f64).clamp(0.0, 100.0)
}

fn storage_metric(used_bytes: u64, total_bytes: u64) -> StorageMetric {
    StorageMetric {
        used_bytes,
        total_bytes,
        usage_percent: percentage(used_bytes, total_bytes),
    }
}

fn percentage(used: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        (used as f64 * 100.0 / total as f64).clamp(0.0, 100.0)
    }
}

#[cfg(windows)]
mod platform {
    use std::mem::size_of;

    use super::{CpuTimes, DiskVolume, percentage};

    const DRIVE_REMOVABLE: u32 = 2;
    const DRIVE_FIXED: u32 = 3;

    #[repr(C)]
    #[derive(Default)]
    struct FileTime {
        low: u32,
        high: u32,
    }

    #[repr(C)]
    struct MemoryStatusEx {
        length: u32,
        memory_load: u32,
        total_physical: u64,
        available_physical: u64,
        total_page_file: u64,
        available_page_file: u64,
        total_virtual: u64,
        available_virtual: u64,
        available_extended_virtual: u64,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetSystemTimes(
            idle_time: *mut FileTime,
            kernel_time: *mut FileTime,
            user_time: *mut FileTime,
        ) -> i32;
        fn GlobalMemoryStatusEx(status: *mut MemoryStatusEx) -> i32;
        fn GetLogicalDrives() -> u32;
        fn GetDriveTypeW(root_path: *const u16) -> u32;
        fn GetDiskFreeSpaceExW(
            directory_name: *const u16,
            free_bytes_available: *mut u64,
            total_bytes: *mut u64,
            total_free_bytes: *mut u64,
        ) -> i32;
    }

    pub(super) fn read_cpu_times() -> Result<CpuTimes, String> {
        let mut idle = FileTime::default();
        let mut kernel = FileTime::default();
        let mut user = FileTime::default();
        // SAFETY: all pointers refer to initialized, writable FILETIME values.
        if unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) } == 0 {
            return Err(format!(
                "无法读取 Windows CPU 计数器：{}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(CpuTimes {
            idle: file_time_value(&idle),
            total: file_time_value(&kernel).saturating_add(file_time_value(&user)),
        })
    }

    pub(super) fn read_memory() -> Result<(u64, u64), String> {
        let mut status = MemoryStatusEx {
            length: size_of::<MemoryStatusEx>() as u32,
            memory_load: 0,
            total_physical: 0,
            available_physical: 0,
            total_page_file: 0,
            available_page_file: 0,
            total_virtual: 0,
            available_virtual: 0,
            available_extended_virtual: 0,
        };
        // SAFETY: status has the documented layout and its length field is initialized.
        if unsafe { GlobalMemoryStatusEx(&mut status) } == 0 {
            return Err(format!(
                "无法读取 Windows 内存状态：{}",
                std::io::Error::last_os_error()
            ));
        }
        Ok((
            status
                .total_physical
                .saturating_sub(status.available_physical),
            status.total_physical,
        ))
    }

    pub(super) fn read_disks() -> Result<Vec<DiskVolume>, String> {
        // SAFETY: GetLogicalDrives has no parameters and does not retain state.
        let drive_mask = unsafe { GetLogicalDrives() };
        if drive_mask == 0 {
            return Err(format!(
                "无法枚举 Windows 磁盘：{}",
                std::io::Error::last_os_error()
            ));
        }

        let mut volumes = Vec::new();
        for index in 0..26_u32 {
            if drive_mask & (1 << index) == 0 {
                continue;
            }
            let letter = char::from_u32(u32::from(b'A') + index).unwrap_or('?');
            let mount_point = format!("{letter}:\\");
            let wide = [
                (u32::from(b'A') + index) as u16,
                u16::from(b':'),
                u16::from(b'\\'),
                0,
            ];
            // SAFETY: wide is a valid, nul-terminated drive root string.
            let drive_type = unsafe { GetDriveTypeW(wide.as_ptr()) };
            if !matches!(drive_type, DRIVE_FIXED | DRIVE_REMOVABLE) {
                continue;
            }

            let mut available = 0_u64;
            let mut total = 0_u64;
            let mut free = 0_u64;
            // SAFETY: wide is nul-terminated and output pointers are valid.
            if unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut available, &mut total, &mut free) }
                == 0
                || total == 0
            {
                continue;
            }
            let used = total.saturating_sub(free);
            volumes.push(DiskVolume {
                name: format!("本地磁盘 {letter}:"),
                mount_point,
                used_bytes: used,
                total_bytes: total,
                usage_percent: percentage(used, total),
            });
        }
        Ok(volumes)
    }

    fn file_time_value(value: &FileTime) -> u64 {
        (u64::from(value.high) << 32) | u64::from(value.low)
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use std::collections::HashSet;
    use std::ffi::CString;
    use std::fs;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;
    use std::path::Path;

    use super::{CpuTimes, DiskVolume, percentage};

    pub(super) fn read_cpu_times() -> Result<CpuTimes, String> {
        let stat = fs::read_to_string("/proc/stat")
            .map_err(|error| format!("无法读取 CPU 状态：{error}"))?;
        let line = stat
            .lines()
            .find(|line| line.starts_with("cpu "))
            .ok_or_else(|| "CPU 状态缺少汇总行".to_owned())?;
        let values = line
            .split_whitespace()
            .skip(1)
            .map(|value| value.parse::<u64>())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("CPU 状态格式无效：{error}"))?;
        if values.len() < 4 {
            return Err("CPU 状态字段不足".to_owned());
        }
        Ok(CpuTimes {
            idle: values[3].saturating_add(values.get(4).copied().unwrap_or(0)),
            total: values
                .iter()
                .fold(0_u64, |total, value| total.saturating_add(*value)),
        })
    }

    pub(super) fn read_memory() -> Result<(u64, u64), String> {
        let meminfo = fs::read_to_string("/proc/meminfo")
            .map_err(|error| format!("无法读取内存状态：{error}"))?;
        let mut total = None;
        let mut available = None;
        let mut free = None;
        for line in meminfo.lines() {
            let mut fields = line.split_whitespace();
            let Some(key) = fields.next() else { continue };
            let Some(value) = fields.next().and_then(|value| value.parse::<u64>().ok()) else {
                continue;
            };
            match key {
                "MemTotal:" => total = Some(value.saturating_mul(1024)),
                "MemAvailable:" => available = Some(value.saturating_mul(1024)),
                "MemFree:" => free = Some(value.saturating_mul(1024)),
                _ => {}
            }
        }
        let total = total.ok_or_else(|| "内存状态缺少 MemTotal".to_owned())?;
        let available = available.or(free).unwrap_or(0);
        Ok((total.saturating_sub(available), total))
    }

    pub(super) fn read_disks() -> Result<Vec<DiskVolume>, String> {
        let mountinfo = fs::read_to_string("/proc/self/mountinfo")
            .map_err(|error| format!("无法读取磁盘挂载信息：{error}"))?;
        let mut seen_devices = HashSet::new();
        let mut volumes = Vec::new();

        for line in mountinfo.lines() {
            let Some((mount_fields, fs_fields)) = line.split_once(" - ") else {
                continue;
            };
            let mount_parts = mount_fields.split_whitespace().collect::<Vec<_>>();
            let fs_parts = fs_fields.split_whitespace().collect::<Vec<_>>();
            if mount_parts.len() < 5 || fs_parts.is_empty() || is_virtual_filesystem(fs_parts[0]) {
                continue;
            }
            let mount_point = decode_mount_path(mount_parts[4]);
            let path = Path::new(&mount_point);
            let Ok(metadata) = fs::metadata(path) else {
                continue;
            };
            if !seen_devices.insert(metadata.dev()) {
                continue;
            }
            let Ok((used, total)) = filesystem_usage(path) else {
                continue;
            };
            if total == 0 {
                continue;
            }
            volumes.push(DiskVolume {
                name: fs_parts.get(1).copied().unwrap_or(fs_parts[0]).to_owned(),
                mount_point,
                used_bytes: used,
                total_bytes: total,
                usage_percent: percentage(used, total),
            });
        }
        volumes.sort_by(|left, right| left.mount_point.cmp(&right.mount_point));
        Ok(volumes)
    }

    fn filesystem_usage(path: &Path) -> Result<(u64, u64), String> {
        let path = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| "磁盘挂载路径包含无效字符".to_owned())?;
        let mut stats = std::mem::MaybeUninit::<libc::statvfs>::zeroed();
        // SAFETY: path is nul-terminated and stats points to writable storage.
        if unsafe { libc::statvfs(path.as_ptr(), stats.as_mut_ptr()) } != 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        // SAFETY: statvfs returned success and initialized stats.
        let stats = unsafe { stats.assume_init() };
        let block_size = stats.f_frsize;
        let total = stats.f_blocks.saturating_mul(block_size);
        let free = stats.f_bfree.saturating_mul(block_size);
        Ok((total.saturating_sub(free), total))
    }

    pub(super) fn decode_mount_path(value: &str) -> String {
        value
            .replace("\\040", " ")
            .replace("\\011", "\t")
            .replace("\\012", "\n")
            .replace("\\134", "\\")
    }

    fn is_virtual_filesystem(kind: &str) -> bool {
        matches!(
            kind,
            "proc"
                | "sysfs"
                | "tmpfs"
                | "devtmpfs"
                | "devpts"
                | "cgroup"
                | "cgroup2"
                | "pstore"
                | "securityfs"
                | "debugfs"
                | "tracefs"
                | "configfs"
                | "hugetlbfs"
                | "mqueue"
                | "autofs"
                | "rpc_pipefs"
                | "fusectl"
                | "binfmt_misc"
                | "efivarfs"
        )
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
mod platform {
    use super::{CpuTimes, DiskVolume};

    pub(super) fn read_cpu_times() -> Result<CpuTimes, String> {
        Err("当前平台暂不支持本机资源监控".to_owned())
    }

    pub(super) fn read_memory() -> Result<(u64, u64), String> {
        Err("当前平台暂不支持本机资源监控".to_owned())
    }

    pub(super) fn read_disks() -> Result<Vec<DiskVolume>, String> {
        Err("当前平台暂不支持本机资源监控".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calculates_cpu_usage_from_two_cumulative_samples() {
        let previous = CpuTimes {
            idle: 1_000,
            total: 4_000,
        };
        let current = CpuTimes {
            idle: 1_025,
            total: 4_100,
        };
        assert_eq!(cpu_usage_percent(previous, current), 75.0);
    }

    #[test]
    fn percentage_handles_empty_and_overcommitted_values() {
        assert_eq!(percentage(10, 0), 0.0);
        assert_eq!(percentage(125, 100), 100.0);
        assert_eq!(percentage(25, 100), 25.0);
    }

    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn collects_host_metrics_without_elevated_permissions() {
        let monitor = SystemMetricsMonitor::default();
        let first = collect_system_metrics(&monitor).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let second = collect_system_metrics(&monitor).unwrap();

        assert!(first.memory.total_bytes > 0);
        assert!(first.disk.total_bytes > 0);
        assert!(!first.disk.volumes.is_empty());
        assert!((0.0..=100.0).contains(&second.cpu.usage_percent));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn decodes_linux_mountinfo_paths() {
        assert_eq!(
            platform::decode_mount_path("/media/My\\040Disk"),
            "/media/My Disk"
        );
    }
}
