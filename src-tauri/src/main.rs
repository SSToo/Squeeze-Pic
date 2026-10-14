#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use base64::Engine;
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use squeeze_pic_core::{ImageInfo, ProcessOptions, ProcessReport};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tauri::{Emitter, Manager, State, ipc::Response};
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_shell::{
    ShellExt,
    process::{CommandChild, CommandEvent},
};

#[derive(Default)]
struct EngineState {
    allowed: Mutex<HashSet<PathBuf>>,
    busy: AtomicBool,
    cancelled: AtomicBool,
    child: Mutex<Option<CommandChild>>,
}
struct Permit(Arc<EngineState>);
impl Permit {
    fn acquire(state: &Arc<EngineState>) -> Result<Self, String> {
        state
            .busy
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| "Another image task is running")?;
        state.cancelled.store(false, Ordering::SeqCst);
        Ok(Self(state.clone()))
    }
}
impl Drop for Permit {
    fn drop(&mut self) {
        self.0.busy.store(false, Ordering::SeqCst);
    }
}
fn authorize(state: &EngineState, path: &str) -> Result<PathBuf, String> {
    let path = std::fs::canonicalize(path).map_err(|e| e.to_string())?;
    if !state.allowed.lock().unwrap().contains(&path) {
        return Err("Please import this image first".into());
    }
    Ok(path)
}
fn scratch(app: &tauri::AppHandle) -> Result<tempfile::TempDir, String> {
    let root = app
        .path()
        .app_cache_dir()
        .map_err(|e| e.to_string())?
        .join("tasks");
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    tempfile::Builder::new()
        .prefix("pic-task-")
        .tempdir_in(root)
        .map_err(|e| e.to_string())
}
async fn worker(
    app: &tauri::AppHandle,
    state: &Arc<EngineState>,
    request: Value,
    path: &Path,
) -> Result<Value, String> {
    if state.cancelled.load(Ordering::SeqCst) {
        return Err("Task cancelled".into());
    }
    let (mut events, mut child) = app
        .shell()
        .sidecar("squeeze-pic-worker")
        .map_err(|e| e.to_string())?
        .arg("worker")
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut payload = serde_json::to_vec(&request).map_err(|e| e.to_string())?;
    payload.push(b'\n');
    // Worker reads one JSON line, so stdin can remain open for this process.
    if let Err(error) = child.write(&payload) {
        let _ = child.kill();
        return Err(error.to_string());
    }
    *state.child.lock().unwrap() = Some(child);
    if state.cancelled.load(Ordering::SeqCst)
        && let Some(child) = state.child.lock().unwrap().take()
    {
        let _ = child.kill();
    }
    let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
    let mut stdout = Vec::new();
    let mut error = String::new();
    let mut terminated = false;
    let result = async {
        loop {
            let event = tokio::time::timeout_at(deadline, events.recv())
                .await
                .map_err(|_| "Image processing timed out")?
                .ok_or("Worker stopped unexpectedly")?;
            match event {
                CommandEvent::Stdout(bytes) => {
                    if stdout.len() + bytes.len() > 1024 * 1024 {
                        return Err("Worker report exceeds limit".into());
                    }
                    stdout.extend(bytes);
                }
                CommandEvent::Stderr(bytes) => {
                    if let Ok(value) = serde_json::from_slice::<Value>(&bytes) {
                        if value.get("phase").is_some() {
                            let _ = app.emit(
                                "pic-progress",
                                json!({"path": path, "phase": value["phase"]}),
                            );
                        } else if let Some(message) = value.get("message").and_then(Value::as_str) {
                            error = message.to_owned();
                        }
                    } else if error.len() < 8192 {
                        error.push_str(&String::from_utf8_lossy(&bytes));
                    }
                }
                CommandEvent::Error(message) => return Err(message),
                CommandEvent::Terminated(exit) => {
                    terminated = true;
                    if exit.code == Some(0) {
                        return serde_json::from_slice(&stdout).map_err(|e| e.to_string());
                    }
                    return Err(if state.cancelled.load(Ordering::SeqCst) {
                        "Task cancelled".into()
                    } else if error.is_empty() {
                        "Image worker failed".into()
                    } else {
                        error
                    });
                }
                _ => {}
            }
        }
    }
    .await;
    if let Some(child) = state.child.lock().unwrap().take()
        && !terminated
    {
        let _ = child.kill();
    }
    if !terminated {
        let drain = async {
            while let Some(event) = events.recv().await {
                if matches!(event, CommandEvent::Terminated(_)) {
                    break;
                }
            }
        };
        tokio::time::timeout(Duration::from_secs(5), drain)
            .await
            .map_err(|_| "Worker did not terminate")?;
    }
    result
}
#[derive(Serialize)]
struct Imported {
    path: PathBuf,
    name: String,
    info: Option<ImageInfo>,
    thumbnail: Option<String>,
    error: Option<String>,
}
#[tauri::command]
async fn import_files(
    app: tauri::AppHandle,
    state: State<'_, Arc<EngineState>>,
    paths: Vec<String>,
) -> Result<Vec<Imported>, String> {
    if paths.len() > 100 {
        return Err("Import up to 100 images at a time".into());
    }
    let state = state.inner().clone();
    let _permit = Permit::acquire(&state)?;
    let mut imported = Vec::new();
    let mut seen = HashSet::new();
    for path in paths {
        if state.cancelled.load(Ordering::SeqCst) {
            break;
        }
        let path = match std::fs::canonicalize(&path) {
            Ok(path) => path,
            Err(e) => {
                imported.push(Imported {
                    name: Path::new(&path)
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into(),
                    path: path.into(),
                    info: None,
                    thumbnail: None,
                    error: Some(e.to_string()),
                });
                continue;
            }
        };
        if !seen.insert(path.clone()) {
            continue;
        }
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let result = worker(
            &app,
            &state,
            json!({"action": "inspect", "input": path}),
            &path,
        )
        .await;
        match result
            .and_then(|value| serde_json::from_value::<ImageInfo>(value).map_err(|e| e.to_string()))
        {
            Ok(info) => {
                state.allowed.lock().unwrap().insert(path.clone());
                let temporary = scratch(&app)?;
                let preview = temporary.path().join("thumbnail.png");
                let thumbnail = if worker(
                    &app,
                    &state,
                    json!({"action": "preview", "input": path, "output": preview, "max_edge": 240}),
                    &path,
                )
                .await
                .is_ok()
                {
                    std::fs::read(&preview)
                        .ok()
                        .filter(|b| b.len() <= 1024 * 1024)
                        .map(|bytes| {
                            format!(
                                "data:image/png;base64,{}",
                                base64::engine::general_purpose::STANDARD.encode(bytes)
                            )
                        })
                } else {
                    None
                };
                imported.push(Imported {
                    path,
                    name,
                    info: Some(info),
                    thumbnail,
                    error: None,
                });
            }
            Err(error) => imported.push(Imported {
                path,
                name,
                info: None,
                thumbnail: None,
                error: Some(error),
            }),
        }
    }
    Ok(imported)
}
fn unique_output(input: &Path, directory: &Path, extension: &str) -> Result<PathBuf, String> {
    if !directory.is_dir() {
        return Err("Output directory does not exist".into());
    }
    let stem = input.file_stem().unwrap_or_default().to_string_lossy();
    for n in 0..10000 {
        let suffix = if n == 0 {
            String::new()
        } else {
            format!("_{n}")
        };
        let path = directory.join(format!("{stem}_squeezed{suffix}.{extension}"));
        if !path.try_exists().map_err(|e| e.to_string())? {
            return Ok(path);
        }
    }
    Err("Too many existing output names".into())
}
fn fingerprint(path: &Path) -> Result<String, String> {
    use std::io::Read;
    let mut input = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 16_384];
    let mut total = 0u64;
    loop {
        let n = input.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > squeeze_pic_core::MAX_INPUT {
            return Err("Original grew beyond the input limit".into());
        }
        hash.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}
#[tauri::command]
async fn process_image(
    app: tauri::AppHandle,
    state: State<'_, Arc<EngineState>>,
    path: String,
    directory: Option<String>,
    options: ProcessOptions,
) -> Result<ProcessReport, String> {
    let state = state.inner().clone();
    let _permit = Permit::acquire(&state)?;
    let input = authorize(&state, &path)?;
    let info: ImageInfo = serde_json::from_value(
        worker(
            &app,
            &state,
            json!({"action": "inspect", "input": input}),
            &input,
        )
        .await?,
    )
    .map_err(|e| e.to_string())?;
    let format = options.format.unwrap_or(info.format);
    let directory = directory
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| input.parent().unwrap().to_path_buf());
    let output = unique_output(&input, &directory, format.extension())?;
    // All worker-owned files live in a host-owned directory, so killing an
    // encoder cannot leave its partial output beside the user's originals.
    let temporary = tempfile::Builder::new()
        .prefix("pic-task-")
        .tempdir_in(&directory)
        .map_err(|e| e.to_string())?;
    let staged = temporary
        .path()
        .join(format!("result.{}", format.extension()));
    let mut report: ProcessReport = serde_json::from_value(
        worker(
            &app,
            &state,
            json!({"action": "convert", "input": input, "output": staged, "options": options}),
            &input,
        )
        .await?,
    )
    .map_err(|e| e.to_string())?;
    if report.output.is_some() {
        if state.cancelled.load(Ordering::SeqCst) {
            return Err("Task cancelled".into());
        }
        let mut committed =
            tempfile::NamedTempFile::new_in(&directory).map_err(|e| e.to_string())?;
        let mut source = std::fs::File::open(&staged).map_err(|e| e.to_string())?;
        std::io::copy(&mut source, committed.as_file_mut()).map_err(|e| e.to_string())?;
        committed.as_file().sync_all().map_err(|e| e.to_string())?;
        if fingerprint(&input)? != report.source_sha256 {
            return Err("Original changed before final export".into());
        }
        if state.cancelled.load(Ordering::SeqCst) {
            return Err("Task cancelled".into());
        }
        committed
            .persist_noclobber(&output)
            .map_err(|e| e.to_string())?;
        report.output = Some(output.clone());
        state
            .allowed
            .lock()
            .unwrap()
            .insert(std::fs::canonicalize(output).map_err(|e| e.to_string())?);
    }
    Ok(report)
}
#[tauri::command]
async fn read_preview(
    app: tauri::AppHandle,
    state: State<'_, Arc<EngineState>>,
    path: String,
) -> Result<Response, String> {
    let state = state.inner().clone();
    let _permit = Permit::acquire(&state)?;
    let input = authorize(&state, &path)?;
    let temporary = scratch(&app)?;
    let output = temporary.path().join("preview.png");
    worker(
        &app,
        &state,
        json!({"action": "preview", "input": input, "output": output, "max_edge": 1600}),
        &input,
    )
    .await?;
    let bytes = std::fs::read(output).map_err(|e| e.to_string())?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err("Preview exceeds limit".into());
    }
    Ok(Response::new(bytes))
}
#[tauri::command]
fn cancel_task(state: State<'_, Arc<EngineState>>) {
    state.cancelled.store(true, Ordering::SeqCst);
    if let Some(child) = state.child.lock().unwrap().take() {
        let _ = child.kill();
    }
}
#[tauri::command]
fn open_result(
    app: tauri::AppHandle,
    state: State<'_, Arc<EngineState>>,
    path: String,
    folder: bool,
) -> Result<(), String> {
    let path = authorize(&state, &path)?;
    if folder {
        app.opener()
            .reveal_item_in_dir(path)
            .map_err(|e| e.to_string())
    } else {
        app.opener()
            .open_path(path.to_str().ok_or("Path cannot be encoded")?, None::<&str>)
            .map_err(|e| e.to_string())
    }
}
fn main() {
    tauri::Builder::default()
        .manage(Arc::new(EngineState::default()))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            import_files,
            process_image,
            read_preview,
            cancel_task,
            open_result
        ])
        .build(tauri::generate_context!())
        .expect("Squeeze Pic initialization failed")
        .run(|app, event| {
            if matches!(event, tauri::RunEvent::Exit) {
                let state = app.state::<Arc<EngineState>>();
                if let Some(child) = state.child.lock().unwrap().take() {
                    let _ = child.kill();
                }
            }
        });
}
