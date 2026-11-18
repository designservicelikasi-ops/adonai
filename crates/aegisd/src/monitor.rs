//! Process and machine monitoring (CPU, RAM) for the VPS, refreshed in a
//! background thread every few seconds.

use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use sysinfo::{CpuRefreshKind, MemoryRefreshKind, ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SystemSnapshot {
    pub ts_ms: i64,
    /// CPU used by the engine process, in % of one core.
    pub process_cpu_pct: f32,
    pub process_memory_mb: f64,
    /// Whole machine.
    pub system_cpu_pct: f32,
    pub system_memory_used_mb: f64,
    pub system_memory_total_mb: f64,
    pub system_uptime_s: u64,
    pub cpus: usize,
}

impl SystemSnapshot {
    pub fn memory_used_pct(&self) -> f64 {
        if self.system_memory_total_mb > 0.0 {
            self.system_memory_used_mb / self.system_memory_total_mb * 100.0
        } else {
            0.0
        }
    }
}

#[derive(Clone)]
pub struct SystemMonitor {
    inner: Arc<Mutex<SystemSnapshot>>,
}

impl SystemMonitor {
    /// Start sampling every `every` (at least one second).
    pub fn start(every: Duration) -> SystemMonitor {
        let inner = Arc::new(Mutex::new(SystemSnapshot::default()));
        let shared = inner.clone();
        let every = every.max(Duration::from_secs(1));
        std::thread::Builder::new()
            .name("aegis-monitor".into())
            .spawn(move || {
                let pid = sysinfo::get_current_pid().ok();
                let mut sys = System::new_with_specifics(RefreshKind::new().with_cpu(CpuRefreshKind::new().with_cpu_usage()).with_memory(MemoryRefreshKind::new().with_ram()));
                loop {
                    sys.refresh_cpu_usage();
                    sys.refresh_memory();
                    let mut snap = SystemSnapshot {
                        ts_ms: aegis_types::now_ms(),
                        system_cpu_pct: sys.global_cpu_usage(),
                        system_memory_used_mb: sys.used_memory() as f64 / 1_048_576.0,
                        system_memory_total_mb: sys.total_memory() as f64 / 1_048_576.0,
                        system_uptime_s: System::uptime(),
                        cpus: sys.cpus().len(),
                        ..Default::default()
                    };
                    if let Some(pid) = pid {
                        sys.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), true, ProcessRefreshKind::new().with_cpu().with_memory());
                        if let Some(p) = sys.process(pid) {
                            snap.process_cpu_pct = p.cpu_usage();
                            snap.process_memory_mb = p.memory() as f64 / 1_048_576.0;
                        }
                    }
                    if let Ok(mut s) = shared.lock() {
                        *s = snap;
                    }
                    std::thread::sleep(every);
                }
            })
            .expect("monitor thread");
        SystemMonitor { inner }
    }

    pub fn snapshot(&self) -> SystemSnapshot {
        self.inner.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn samples_the_real_process() {
        let m = super::SystemMonitor::start(std::time::Duration::from_secs(1));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let s = m.snapshot();
            if s.ts_ms > 0 && s.process_memory_mb > 0.0 {
                assert!(s.system_memory_total_mb > s.process_memory_mb);
                assert!(s.cpus > 0);
                assert!(s.memory_used_pct() > 0.0 && s.memory_used_pct() <= 100.0);
                break;
            }
            assert!(std::time::Instant::now() < deadline, "no sample");
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
}
