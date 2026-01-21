// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// Learn more about Tauri commands at https://tauri.app/v1/guides/features/command
use serde::Serialize;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Serialize)]
struct TodoItem {
    id: u64,
    text: String,
    completed: bool,
    created_at: i64,
}

struct AppState(Mutex<Vec<TodoItem>>);

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

#[tauri::command]
fn get_tasks(state: tauri::State<'_, AppState>) -> Vec<TodoItem> {
    state.0.lock().unwrap().clone()
}

#[tauri::command]
fn add_task(text: String, state: tauri::State<'_, AppState>) -> Vec<TodoItem> {
    let mut tasks = state.0.lock().unwrap();
    let item = TodoItem {
        id: now_millis(),
        text,
        completed: false,
        created_at: now_secs(),
    };

    // 최신이 위로
    tasks.insert(0, item);
    tasks.clone()
}

#[tauri::command]
fn toggle_task(id: u64, state: tauri::State<'_, AppState>) -> Vec<TodoItem> {
    let mut tasks = state.0.lock().unwrap();
    if let Some(t) = tasks.iter_mut().find(|t| t.id == id) {
        t.completed = !t.completed;
    }
    tasks.clone()
}

#[tauri::command]
fn delete_task(id: u64, state: tauri::State<'_, AppState>) -> Vec<TodoItem> {
    let mut tasks = state.0.lock().unwrap();
    tasks.retain(|t| t.id != id);
    tasks.clone()
}

fn main() {
    tauri::Builder::default()
        .manage(AppState(Mutex::new(Vec::new())))
        .invoke_handler(tauri::generate_handler![get_tasks, add_task, toggle_task, delete_task])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}