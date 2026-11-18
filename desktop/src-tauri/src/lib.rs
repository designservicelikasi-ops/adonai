//! Native side of AEGIS Desktop.
//!
//! The application does not contain the engine: it is a client of the engine's
//! control API (local or on a VPS). Network calls are made here, in Rust, with
//! certificate pinning (`aegis-client`), never from the web view. The local
//! engine, when used, is started as a DETACHED process: closing the window never
//! stops the engine or its positions.

use aegis_client::{probe_fingerprint, Client, ClientError, Endpoint};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager, State};

#[derive(Default)]
struct AppState {
    client: Mutex<Option<Arc<Client>>>,
    endpoint: Mutex<Option<Endpoint>>,
}

#[derive(Serialize)]
struct ProfileView {
    url: String,
    fingerprint: String,
    has_token: bool,
    local: bool,
}

#[derive(Serialize, Deserialize, Clone)]
struct Profile {
    endpoint: Endpoint,
    #[serde(default)]
    local: bool,
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn client_error(e: ClientError) -> String {
    match e {
        ClientError::Unauthorized => "UNAUTHORIZED: jeton refusé par le moteur".into(),
        ClientError::Transport(m) => format!("TRANSPORT: {m}"),
        other => other.to_string(),
    }
}

fn config_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let d = app.path().app_config_dir().map_err(err)?;
    std::fs::create_dir_all(&d).map_err(err)?;
    Ok(d)
}

fn profile_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(config_dir(app)?.join("connection.json"))
}

fn write_private(path: &Path, content: &str) -> Result<(), String> {
    use std::io::Write;
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    o.open(path).and_then(|mut f| f.write_all(content.as_bytes())).map_err(err)
}

fn connect(state: &State<AppState>, ep: Endpoint) -> Result<Value, String> {
    let c = Client::new(&ep).map_err(client_error)?;
    // A wrong token or certificate is detected here, before anything is saved.
    c.get("/api/v1/summary").map_err(client_error)?;
    let health = c.health().map_err(client_error)?;
    *state.client.lock().map_err(err)? = Some(Arc::new(c));
    *state.endpoint.lock().map_err(err)? = Some(ep);
    Ok(health)
}

#[tauri::command]
fn profile_load(app: AppHandle, state: State<AppState>) -> Result<Option<ProfileView>, String> {
    let path = profile_path(&app)?;
    let Ok(text) = std::fs::read_to_string(&path) else { return Ok(None) };
    let p: Profile = serde_json::from_str(&text).map_err(err)?;
    let view = ProfileView { url: p.endpoint.url.clone(), fingerprint: p.endpoint.fingerprint.clone(), has_token: !p.endpoint.token.is_empty(), local: p.local };
    // Try to connect; the interface shows the error if the engine is not reachable.
    if let Ok(c) = Client::new(&p.endpoint) {
        *state.client.lock().map_err(err)? = Some(Arc::new(c));
        *state.endpoint.lock().map_err(err)? = Some(p.endpoint);
    }
    Ok(Some(view))
}

#[tauri::command]
fn profile_save(app: AppHandle, state: State<AppState>, url: String, token: String, fingerprint: String) -> Result<Value, String> {
    let ep = Endpoint { url: url.trim().to_string(), token: token.trim().to_string(), fingerprint: fingerprint.trim().to_string() };
    let health = connect(&state, ep.clone())?;
    write_private(&profile_path(&app)?, &serde_json::to_string_pretty(&Profile { endpoint: ep, local: false }).map_err(err)?)?;
    Ok(health)
}

#[tauri::command]
fn profile_forget(app: AppHandle, state: State<AppState>) -> Result<(), String> {
    let _ = std::fs::remove_file(profile_path(&app)?);
    *state.client.lock().map_err(err)? = None;
    *state.endpoint.lock().map_err(err)? = None;
    Ok(())
}

#[tauri::command]
async fn fingerprint_probe(url: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || probe_fingerprint(url.trim()).map_err(client_error)).await.map_err(err)?
}

fn current(state: &State<AppState>) -> Result<Arc<Client>, String> {
    state.client.lock().map_err(err)?.clone().ok_or_else(|| "NOT_CONFIGURED: aucun moteur configuré".to_string())
}

#[tauri::command]
async fn api_get(state: State<'_, AppState>, path: String) -> Result<Value, String> {
    if !path.starts_with("/api/v1/") || path.contains("..") {
        return Err("chemin d'API refusé".into());
    }
    let c = current(&state)?;
    tauri::async_runtime::spawn_blocking(move || if path == "/api/v1/health" { c.health() } else { c.get(&path) }.map_err(client_error))
    .await
    .map_err(err)?
}

#[tauri::command]
async fn api_command(state: State<'_, AppState>, command: Value) -> Result<String, String> {
    let c = current(&state)?;
    tauri::async_runtime::spawn_blocking(move || c.command(&command).map_err(client_error)).await.map_err(err)?
}

// ------------------------------------------------------------ local engine

fn engine_exe() -> Result<PathBuf, String> {
    let dir = std::env::current_exe().map_err(err)?.parent().map(Path::to_path_buf).ok_or("dossier de l'application introuvable")?;
    let name = if cfg!(windows) { "aegisd.exe" } else { "aegisd" };
    let p = dir.join(name);
    if p.exists() {
        Ok(p)
    } else {
        Err(format!("moteur local introuvable ({}). Installez AEGIS avec son moteur, ou connectez-vous à un moteur distant.", p.display()))
    }
}

fn local_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let d = app.path().app_data_dir().map_err(err)?.join("engine");
    std::fs::create_dir_all(&d).map_err(err)?;
    Ok(d)
}

const LOCAL_PORT: u16 = 7879;

#[derive(Serialize)]
struct LocalStatus {
    installed: bool,
    running: bool,
    data_dir: String,
    detail: String,
}

#[tauri::command]
async fn local_engine_status(app: AppHandle) -> Result<LocalStatus, String> {
    let dir = local_dir(&app)?;
    let installed = engine_exe().is_ok();
    let url = format!("https://127.0.0.1:{LOCAL_PORT}");
    let running = tauri::async_runtime::spawn_blocking(move || probe_fingerprint(&url).is_ok()).await.map_err(err)?;
    Ok(LocalStatus {
        installed,
        running,
        data_dir: dir.display().to_string(),
        detail: if running { "moteur local en cours d'exécution".into() } else if installed { "moteur local installé, arrêté".into() } else { engine_exe().err().unwrap_or_default() },
    })
}

/// Start the bundled engine as a detached process and pair with it automatically.
#[tauri::command]
async fn local_engine_start(app: AppHandle, state: State<'_, AppState>, simulator: bool) -> Result<Value, String> {
    let exe = engine_exe()?;
    let dir = local_dir(&app)?;
    let config = dir.join("aegis.toml");
    if !config.exists() {
        let data = dir.join("data");
        let toml = format!(
            "# Moteur local d'AEGIS Desktop (créé automatiquement).\n[engine]\ndata_dir = {:?}\n\n[api]\nenabled = true\nlisten = \"127.0.0.1:{LOCAL_PORT}\"\ntls = true\n",
            data.display().to_string()
        );
        std::fs::write(&config, toml).map_err(err)?;
    }
    let url = format!("https://127.0.0.1:{LOCAL_PORT}");
    let probe = url.clone();
    let already = tauri::async_runtime::spawn_blocking(move || probe_fingerprint(&probe).is_ok()).await.map_err(err)?;
    if !already {
        let mut cmd = std::process::Command::new(&exe);
        cmd.arg("--config").arg(&config).arg("run");
        if simulator {
            // Test adapter: synthetic prices on EURUSD.sim, clearly labelled everywhere.
            cmd.arg("--simulator");
        }
        cmd.current_dir(&dir).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const DETACHED_PROCESS: u32 = 0x0000_0008;
            const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
        }
        cmd.spawn().map_err(|e| format!("démarrage du moteur impossible: {e}"))?;
    }
    // Pair automatically: same machine, read the token file and the certificate.
    let token_file = dir.join("data").join("api.token");
    let (u2, tf) = (url.clone(), token_file.clone());
    let (token, fingerprint) = tauri::async_runtime::spawn_blocking(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            if let (Ok(t), Ok(fp)) = (std::fs::read_to_string(&tf), probe_fingerprint(&u2)) {
                return Ok((t.trim().to_string(), fp));
            }
            if std::time::Instant::now() > deadline {
                return Err("le moteur local ne répond pas (voir les journaux dans le dossier de données)".to_string());
            }
            std::thread::sleep(std::time::Duration::from_millis(300));
        }
    })
    .await
    .map_err(err)??;
    let ep = Endpoint { url, token, fingerprint };
    let health = connect(&state, ep.clone())?;
    write_private(&profile_path(&app)?, &serde_json::to_string_pretty(&Profile { endpoint: ep, local: true }).map_err(err)?)?;
    Ok(health)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            profile_load,
            profile_save,
            profile_forget,
            fingerprint_probe,
            api_get,
            api_command,
            local_engine_status,
            local_engine_start
        ])
        .run(tauri::generate_context!())
        .expect("error while running AEGIS Desktop");
}
