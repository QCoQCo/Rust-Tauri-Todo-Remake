// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// Learn more about Tauri commands at https://tauri.app/v1/guides/features/command
mod storage;

use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::Manager;

#[derive(Clone, Serialize, Deserialize)]
struct TodoItem {
    id: u64,
    text: String,
    completed: bool,
    created_at: i64,
}

#[derive(Clone, Serialize, Deserialize, Default)]
struct StopwatchState {
    elapsed_ms: u64,
    lap_totals_ms: Vec<u64>,
}

#[derive(Clone, Serialize, Deserialize)]
struct AppData {
    v: u32,
    tasks: Vec<TodoItem>,
    stopwatch: Option<StopwatchState>,
}

impl Default for AppData {
    fn default() -> Self {
        Self {
            v: 1,
            tasks: Vec::new(),
            stopwatch: None,
        }
    }
}

struct AppState(Mutex<AppData>);
struct StorageState(Mutex<Option<String>>);

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn persist(app: &tauri::AppHandle, data: &AppData) {
    match serde_json::to_vec(data) {
        Ok(bytes) => {
            if let Err(e) = storage::save_encrypted(app, &bytes) {
                eprintln!("persist failed: {e}");
            }
        }
        Err(e) => eprintln!("persist serialize failed: {e}"),
    }
}

#[tauri::command]
fn get_tasks(state: tauri::State<'_, AppState>) -> Vec<TodoItem> {
    state.0.lock().unwrap().tasks.clone()
}

#[tauri::command]
fn add_task(text: String, state: tauri::State<'_, AppState>, app: tauri::AppHandle) -> Vec<TodoItem> {
    let mut data = state.0.lock().unwrap();
    let item = TodoItem {
        id: now_millis(),
        text,
        completed: false,
        created_at: now_secs(),
    };

    // 최신이 위로
    data.tasks.insert(0, item);
    let tasks = data.tasks.clone();
    let snapshot = data.clone();
    drop(data);
    persist(&app, &snapshot);
    tasks
}

#[tauri::command]
fn toggle_task(id: u64, state: tauri::State<'_, AppState>, app: tauri::AppHandle) -> Vec<TodoItem> {
    let mut data = state.0.lock().unwrap();
    if let Some(t) = data.tasks.iter_mut().find(|t| t.id == id) {
        t.completed = !t.completed;
    }
    let tasks = data.tasks.clone();
    let snapshot = data.clone();
    drop(data);
    persist(&app, &snapshot);
    tasks
}

#[tauri::command]
fn delete_task(id: u64, state: tauri::State<'_, AppState>, app: tauri::AppHandle) -> Vec<TodoItem> {
    let mut data = state.0.lock().unwrap();
    data.tasks.retain(|t| t.id != id);
    let tasks = data.tasks.clone();
    let snapshot = data.clone();
    drop(data);
    persist(&app, &snapshot);
    tasks
}

#[tauri::command]
fn get_stopwatch_state(state: tauri::State<'_, AppState>) -> Option<StopwatchState> {
    state.0.lock().unwrap().stopwatch.clone()
}

#[tauri::command]
fn set_stopwatch_state(
    stopwatch: StopwatchState,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Option<StopwatchState> {
    let mut data = state.0.lock().unwrap();
    data.stopwatch = Some(stopwatch);
    let out = data.stopwatch.clone();
    let snapshot = data.clone();
    drop(data);
    persist(&app, &snapshot);
    out
}

#[tauri::command]
fn clear_stopwatch_state(state: tauri::State<'_, AppState>, app: tauri::AppHandle) -> bool {
    let mut data = state.0.lock().unwrap();
    data.stopwatch = None;
    let snapshot = data.clone();
    drop(data);
    persist(&app, &snapshot);
    true
}

fn main() {
    tauri::Builder::default()
        .manage(AppState(Mutex::new(AppData::default())))
        .manage(StorageState(Mutex::new(None)))
        .setup(|app| {
            match storage::load_encrypted(&app.handle()) {
                Ok(Some(bytes)) => match serde_json::from_slice::<AppData>(&bytes) {
                    Ok(loaded) => {
                        let state = app.state::<AppState>();
                        let mut guard = state.0.lock().unwrap();
                        *guard = loaded;
                    }
                    Err(e) => {
                        eprintln!("failed to parse stored data: {e}");
                        let err_state = app.state::<StorageState>();
                        *err_state.0.lock().unwrap() = Some(format!("저장 데이터 파싱 실패: {e}"));
                    }
                },
                Ok(None) => {}
                Err(e) => {
                    eprintln!("failed to load encrypted data: {e}");
                    let err_state = app.state::<StorageState>();
                    *err_state.0.lock().unwrap() = Some(format!("암호화 데이터 복호화 실패: {e}"));
                }
            };
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_tasks,
            add_task,
            toggle_task,
            delete_task,
            get_stopwatch_state,
            set_stopwatch_state,
            clear_stopwatch_state,
            get_storage_error,
            reset_storage
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[tauri::command]
fn get_storage_error(state: tauri::State<'_, StorageState>) -> Option<String> {
    state.0.lock().unwrap().clone()
}

#[tauri::command]
fn reset_storage(
    data_state: tauri::State<'_, AppState>,
    err_state: tauri::State<'_, StorageState>,
    app: tauri::AppHandle,
) -> bool {
    if let Err(e) = storage::reset_storage(&app) {
        eprintln!("reset_storage failed: {e}");
        return false;
    }

    *data_state.0.lock().unwrap() = AppData::default();
    *err_state.0.lock().unwrap() = None;
    true
}