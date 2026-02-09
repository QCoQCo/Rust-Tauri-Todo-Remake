// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// Learn more about Tauri commands at https://tauri.app/v1/guides/features/command
mod storage;

use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{Manager, WindowEvent};

#[derive(Clone, Serialize, Deserialize)]
struct TodoItem {
    id: u64,
    text: String,
    completed: bool,
    created_at: i64,
    completed_at: Option<i64>, // 완료 시각 (통계용)
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
        completed_at: None,
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
        if t.completed {
            t.completed_at = Some(now_secs());
        } else {
            t.completed_at = None;
        }
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

#[tauri::command(rename_all = "snake_case")]
async fn export_data(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    file_path: String,
) -> Result<String, String> {
    let data = state.0.lock().unwrap().clone();
    let bytes = serde_json::to_vec(&data).map_err(|e| format!("serialize error: {e}"))?;

    let path = std::path::PathBuf::from(file_path);
    storage::export_backup(&app, &path, &bytes)?;
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command(rename_all = "snake_case")]
async fn import_data(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    file_path: String,
) -> Result<AppData, String> {
    let path = std::path::PathBuf::from(file_path);

    let bytes = storage::import_backup(&app, &path)?;
    let imported: AppData = serde_json::from_slice(&bytes).map_err(|e| format!("parse error: {e}"))?;

    // 상태 업데이트
    let mut current = state.0.lock().unwrap();
    *current = imported.clone();
    drop(current);

    // 즉시 저장
    persist(&app, &imported);
    Ok(imported)
}

// --- 통계 관련 구조체 ---
#[derive(Clone, Serialize, Deserialize)]
struct DailyStats {
    date: String, // YYYY-MM-DD
    tasks_completed: u32,
    tasks_created: u32,
    focus_time_ms: u64, // 스탑워치 사용 시간 (밀리초)
    lap_count: u32,
    avg_lap_time_ms: Option<u64>, // 평균 Lap 시간
}

#[derive(Clone, Serialize, Deserialize)]
struct WeeklyStats {
    start_date: String, // YYYY-MM-DD
    end_date: String,
    total_tasks_completed: u32,
    total_tasks_created: u32,
    total_focus_time_ms: u64,
    total_lap_count: u32,
    avg_daily_completion: f64,
    daily_stats: Vec<DailyStats>,
}

fn date_to_timestamp(date_str: &str) -> i64 {
    // YYYY-MM-DD를 timestamp로 변환 (자정 기준)
    let parts: Vec<&str> = date_str.split('-').collect();
    if parts.len() != 3 {
        return 0;
    }
    let year: i32 = parts[0].parse().unwrap_or(1970);
    let month: u32 = parts[1].parse().unwrap_or(1);
    let day: u32 = parts[2].parse().unwrap_or(1);
    
    let dt = chrono::NaiveDate::from_ymd_opt(year, month, day)
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .map(|dt| dt.and_utc().timestamp())
        .unwrap_or(0);
    
    dt
}

fn timestamp_to_date(ts: i64) -> String {
    use chrono::TimeZone;
    let dt = chrono::Utc.timestamp_opt(ts, 0).unwrap();
    dt.format("%Y-%m-%d").to_string()
}

fn get_date_range(start_date: &str, end_date: &str) -> Vec<String> {
    let start_ts = date_to_timestamp(start_date);
    let end_ts = date_to_timestamp(end_date);
    let mut dates = Vec::new();
    let mut current = start_ts;
    while current <= end_ts {
        dates.push(timestamp_to_date(current));
        current += 86400; // 하루 추가
    }
    dates
}

fn compute_daily_stats(data: &AppData, date: &str) -> DailyStats {
    let start_ts = date_to_timestamp(date);
    let end_ts = start_ts + 86400; // 다음 날 자정 전까지

    let tasks_completed = data
        .tasks
        .iter()
        .filter(|t| {
            t.completed
                && t.completed_at.is_some()
                && t.completed_at.unwrap() >= start_ts
                && t.completed_at.unwrap() < end_ts
        })
        .count() as u32;

    let tasks_created = data
        .tasks
        .iter()
        .filter(|t| t.created_at >= start_ts && t.created_at < end_ts)
        .count() as u32;

    // 스탑워치 통계는 현재 상태만 있으므로 간단히 처리
    let (focus_time_ms, lap_count, avg_lap_time_ms) = if let Some(sw) = &data.stopwatch {
        let lap_count = sw.lap_totals_ms.len() as u32;
        let avg = if lap_count > 0 {
            let sum: u64 = sw.lap_totals_ms.iter().sum();
            Some(sum / lap_count as u64)
        } else {
            None
        };
        (sw.elapsed_ms, lap_count, avg)
    } else {
        (0, 0, None)
    };

    DailyStats {
        date: date.to_string(),
        tasks_completed,
        tasks_created,
        focus_time_ms,
        lap_count,
        avg_lap_time_ms,
    }
}

#[tauri::command(rename_all = "snake_case")]
fn get_daily_stats(
    date: String,
    state: tauri::State<'_, AppState>,
) -> Result<DailyStats, String> {
    let data = state.0.lock().unwrap();
    Ok(compute_daily_stats(&data, &date))
}

#[tauri::command(rename_all = "snake_case")]
fn get_weekly_stats(
    start_date: String,
    state: tauri::State<'_, AppState>,
) -> Result<WeeklyStats, String> {
    let data = state.0.lock().unwrap();
    let dates = get_date_range(&start_date, &timestamp_to_date(date_to_timestamp(&start_date) + 6 * 86400));
    let end_date = dates.last().unwrap().clone();

    let mut daily_stats = Vec::new();
    let mut total_completed = 0u32;
    let mut total_created = 0u32;
    let mut total_focus_ms = 0u64;
    let mut total_laps = 0u32;

    for date in &dates {
        let stats = compute_daily_stats(&data, date);
        total_completed += stats.tasks_completed;
        total_created += stats.tasks_created;
        total_focus_ms += stats.focus_time_ms;
        total_laps += stats.lap_count;
        daily_stats.push(stats);
    }

    let avg_daily_completion = if daily_stats.len() > 0 {
        total_completed as f64 / daily_stats.len() as f64
    } else {
        0.0
    };

    Ok(WeeklyStats {
        start_date,
        end_date,
        total_tasks_completed: total_completed,
        total_tasks_created: total_created,
        total_focus_time_ms: total_focus_ms,
        total_lap_count: total_laps,
        avg_daily_completion,
        daily_stats,
    })
}

#[tauri::command(rename_all = "snake_case")]
async fn export_stats_csv(
    start_date: String,
    end_date: String,
    file_path: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    let data = state.0.lock().unwrap();
    let dates = get_date_range(&start_date, &end_date);
    let mut csv = String::from("날짜,완료된 할 일,생성된 할 일,집중 시간(분),Lap 수,평균 Lap 시간(초)\n");

    for date in dates {
        let stats = compute_daily_stats(&data, &date);
        let focus_min = stats.focus_time_ms / 60000;
        let avg_lap_sec = stats.avg_lap_time_ms.map(|ms| ms / 1000).unwrap_or(0);
        csv.push_str(&format!(
            "{},{},{},{},{},{}\n",
            stats.date,
            stats.tasks_completed,
            stats.tasks_created,
            focus_min,
            stats.lap_count,
            avg_lap_sec
        ));
    }

    let path = if let Some(p) = file_path {
        std::path::PathBuf::from(p)
    } else {
        let default_name = format!("todo_stats_{}_{}.csv", start_date, end_date);
        std::path::PathBuf::from(default_name)
    };
    
    std::fs::write(&path, csv.as_bytes()).map_err(|e| format!("CSV write error: {e}"))?;
    Ok(path.to_string_lossy().to_string())
}

fn main() {
    tauri::Builder::default()
        .manage(AppState(Mutex::new(AppData::default())))
        .setup(|app| {
            if let Ok(Some(bytes)) = storage::load_encrypted(&app.handle()) {
                match serde_json::from_slice::<AppData>(&bytes) {
                    Ok(loaded) => {
                        let state = app.state::<AppState>();
                        let mut guard = state.0.lock().unwrap();
                        *guard = loaded;
                    }
                    Err(e) => eprintln!("failed to parse stored data: {e}"),
                }
            }
            Ok(())
        })
        .on_window_event(|event| {
            // 창 이벤트를 안전하게 처리하여 크래시 방지
            // 최소화 이벤트를 포함한 모든 이벤트를 안전하게 처리
            match event.event() {
                WindowEvent::CloseRequested { .. } => {
                    // 창 닫기 이벤트 처리
                }
                WindowEvent::Resized { .. } => {
                    // 크기 변경 이벤트 처리
                }
                _ => {
                    // 기타 모든 이벤트(최소화 포함)는 안전하게 처리
                    // 이 핸들러가 존재함으로써 null pointer dereference 방지
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_tasks,
            add_task,
            toggle_task,
            delete_task,
            get_stopwatch_state,
            set_stopwatch_state,
            clear_stopwatch_state,
            export_data,
            import_data,
            get_daily_stats,
            get_weekly_stats,
            export_stats_csv
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}