//! Running the engine as a system service: systemd on Linux, the Service
//! Control Manager on Windows. Both restart the engine automatically when it
//! stops unexpectedly; the engine always restarts in MONITOR.

use std::path::Path;

pub const SERVICE_NAME: &str = "AegisEngine";
pub const SERVICE_DISPLAY_NAME: &str = "AEGIS Engine";

/// systemd unit for Linux. `Restart=always` + `RestartSec` give automatic recovery.
pub fn systemd_unit(exe: &Path, config: &Path, workdir: &Path, user: &str) -> String {
    format!(
        "[Unit]
Description=AEGIS trading engine (headless)
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User={user}
WorkingDirectory={wd}
ExecStart={exe} --config {cfg} run
Restart=always
RestartSec=5
# The engine stops cleanly on SIGINT (database closed, audit entry written).
KillSignal=SIGINT
TimeoutStopSec=30
# Secrets: put AEGIS_SHARED_KEY / AEGIS_API_TOKEN here or let the engine create its key files.
EnvironmentFile=-/etc/aegis/aegis.env
NoNewPrivileges=true
ProtectSystem=full
PrivateTmp=true

[Install]
WantedBy=multi-user.target
",
        exe = exe.display(),
        cfg = config.display(),
        wd = workdir.display(),
    )
}

#[cfg(windows)]
pub mod windows {
    //! Windows service (NON VÉRIFIÉ until built and run on Windows: see TESTING.md).
    use super::*;
    use std::ffi::OsString;
    use std::sync::mpsc;
    use std::time::Duration;
    use windows_service::service::{
        ServiceAccess, ServiceAction, ServiceActionType, ServiceControl, ServiceControlAccept, ServiceErrorControl, ServiceExitCode,
        ServiceFailureActions, ServiceFailureResetPeriod, ServiceInfo, ServiceStartType, ServiceState, ServiceStatus, ServiceType,
    };
    use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
    use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};
    use windows_service::{define_windows_service, service_dispatcher};

    /// Register the service: automatic start, restart after 5 s on failure.
    pub fn install(config: &Path) -> anyhow::Result<()> {
        let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE)?;
        let exe = std::env::current_exe()?;
        let info = ServiceInfo {
            name: OsString::from(SERVICE_NAME),
            display_name: OsString::from(SERVICE_DISPLAY_NAME),
            service_type: ServiceType::OWN_PROCESS,
            start_type: ServiceStartType::AutoStart,
            error_control: ServiceErrorControl::Normal,
            executable_path: exe,
            launch_arguments: vec![OsString::from("--config"), config.as_os_str().to_owned(), OsString::from("service-run")],
            dependencies: vec![],
            account_name: None,
            account_password: None,
        };
        let service = manager.create_service(&info, ServiceAccess::CHANGE_CONFIG | ServiceAccess::START)?;
        service.set_description("Moteur AEGIS: risque, décisions, exécution, journal. Démarre toujours en MONITOR.")?;
        let restart = ServiceAction { action_type: ServiceActionType::Restart, delay: Duration::from_secs(5) };
        service.update_failure_actions(ServiceFailureActions {
            reset_period: ServiceFailureResetPeriod::After(Duration::from_secs(86_400)),
            reboot_msg: None,
            command: None,
            actions: Some(vec![restart.clone(), restart.clone(), restart]),
        })?;
        service.set_failure_actions_on_non_crash_failures(true)?;
        Ok(())
    }

    pub fn uninstall() -> anyhow::Result<()> {
        let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
        let service = manager.open_service(SERVICE_NAME, ServiceAccess::QUERY_STATUS | ServiceAccess::STOP | ServiceAccess::DELETE)?;
        if service.query_status()?.current_state != ServiceState::Stopped {
            let _ = service.stop();
        }
        service.delete()?;
        Ok(())
    }

    static CONFIG: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

    define_windows_service!(ffi_service_main, service_main);

    /// Entry point used by the SCM (`aegisd --config X service-run`). Blocks until the service stops.
    pub fn run(config: &Path) -> anyhow::Result<()> {
        let _ = CONFIG.set(config.to_path_buf());
        service_dispatcher::start(SERVICE_NAME, ffi_service_main)?;
        Ok(())
    }

    fn status(state: ServiceState, exit: u32) -> ServiceStatus {
        ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: state,
            controls_accepted: if state == ServiceState::Running { ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN } else { ServiceControlAccept::empty() },
            exit_code: ServiceExitCode::Win32(exit),
            checkpoint: 0,
            wait_hint: Duration::from_secs(if state == ServiceState::Running { 0 } else { 30 }),
            process_id: None,
        }
    }

    fn service_main(_args: Vec<OsString>) {
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let handler = move |event| match event {
            ServiceControl::Stop | ServiceControl::Shutdown => {
                let _ = stop_tx.send(());
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        };
        let Ok(handle) = service_control_handler::register(SERVICE_NAME, handler) else { return };
        let _ = handle.set_service_status(status(ServiceState::StartPending, 0));
        let config = CONFIG.get().cloned().unwrap_or_else(|| std::path::PathBuf::from("config/aegis.toml"));
        let result = (|| -> anyhow::Result<()> {
            let cfg = aegis_core::EngineConfig::load(&config)?;
            let rt = tokio::runtime::Runtime::new()?;
            let _ = handle.set_service_status(status(ServiceState::Running, 0));
            rt.block_on(crate::daemon::serve(cfg, false, async move {
                let _ = tokio::task::spawn_blocking(move || stop_rx.recv()).await;
            }))
        })();
        let code = if result.is_ok() { 0 } else { 1 };
        if let Err(e) = result {
            tracing::error!("service stopped with an error: {e:#}");
        }
        let _ = handle.set_service_status(status(ServiceState::Stopped, code));
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn systemd_unit_restarts_and_stops_cleanly() {
        let u = super::systemd_unit(std::path::Path::new("/opt/aegis/aegisd"), std::path::Path::new("/etc/aegis/aegis.toml"), std::path::Path::new("/var/lib/aegis"), "aegis");
        assert!(u.contains("ExecStart=/opt/aegis/aegisd --config /etc/aegis/aegis.toml run"));
        assert!(u.contains("Restart=always") && u.contains("KillSignal=SIGINT") && u.contains("User=aegis"));
    }
}
