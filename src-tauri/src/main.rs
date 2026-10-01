// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// Learn more about Tauri commands at https://tauri.app/v1/guides/features/command
mod storage;

use chrono::{Days, Local, NaiveDate, TimeZone};
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

// 시작 시 저장 데이터를 불러오지 못한 경우 (프론트에서 한 번 알림)
#[derive(Clone, Serialize)]
struct LoadFailure {
    reason: String,
    preserved_path: Option<String>, // 원본을 옮겨 보존한 위치
    writes_blocked: bool,           // 보존에 실패해 저장을 막았는지
}

struct LoadFailureState(Mutex<Option<LoadFailure>>);

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
fn take_load_failure(state: tauri::State<'_, LoadFailureState>) -> Option<LoadFailure> {
    state.0.lock().unwrap().take()
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

fn parse_date(date_str: &str) -> Result<NaiveDate, String> {
    // YYYY-MM-DD
    NaiveDate::parse_from_str(date_str.trim(), "%Y-%m-%d")
        .map_err(|e| format!("invalid date '{date_str}': {e}"))
}

/// 주어진 시간대에서 그 날짜가 시작되는 순간 (epoch 초)
fn day_start<Tz: TimeZone>(tz: &Tz, date: NaiveDate) -> i64 {
    // DST로 자정이 건너뛰어지는 지역이면 그날 처음 존재하는 정각을 사용
    (0..24)
        .filter_map(|h| date.and_hms_opt(h, 0, 0))
        .find_map(|naive| tz.from_local_datetime(&naive).earliest())
        .map(|dt| dt.timestamp())
        .unwrap_or_else(|| date.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp())
}

/// start ~ end (양 끝 포함). start가 end보다 늦으면 빈 목록
fn date_range(start: NaiveDate, end: NaiveDate) -> Vec<NaiveDate> {
    start.iter_days().take_while(|d| *d <= end).collect()
}

fn compute_daily_stats<Tz: TimeZone>(data: &AppData, date: NaiveDate, tz: &Tz) -> DailyStats {
    // 하루 길이를 86400초로 가정하지 않고 다음 날 시작 시각까지로 계산 (DST 대응)
    let start_ts = day_start(tz, date);
    let end_ts = date.succ_opt().map_or(i64::MAX, |next| day_start(tz, next));

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
        date: date.format("%Y-%m-%d").to_string(),
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
    let date = parse_date(&date)?;
    let data = state.0.lock().unwrap();
    Ok(compute_daily_stats(&data, date, &Local))
}

#[tauri::command(rename_all = "snake_case")]
fn get_weekly_stats(
    start_date: String,
    state: tauri::State<'_, AppState>,
) -> Result<WeeklyStats, String> {
    let start = parse_date(&start_date)?;
    let end = start
        .checked_add_days(Days::new(6))
        .ok_or_else(|| format!("date out of range: {start_date}"))?;
    let end_date = end.format("%Y-%m-%d").to_string();
    let dates = date_range(start, end);
    let data = state.0.lock().unwrap();

    let mut daily_stats = Vec::new();
    let mut total_completed = 0u32;
    let mut total_created = 0u32;
    let mut total_focus_ms = 0u64;
    let mut total_laps = 0u32;

    for date in dates {
        let stats = compute_daily_stats(&data, date, &Local);
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
    let dates = date_range(parse_date(&start_date)?, parse_date(&end_date)?);
    let data = state.0.lock().unwrap();
    let mut csv = String::from("날짜,완료된 할 일,생성된 할 일,집중 시간(분),Lap 수,평균 Lap 시간(초)\n");

    for date in dates {
        let stats = compute_daily_stats(&data, date, &Local);
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
        .manage(LoadFailureState(Mutex::new(None)))
        .setup(|app| {
            let handle = app.handle();
            let loaded = storage::load_encrypted(&handle).and_then(|bytes| {
                bytes
                    .map(|b| {
                        serde_json::from_slice::<AppData>(&b)
                            .map_err(|e| format!("stored data parse error: {e}"))
                    })
                    .transpose()
            });
            match loaded {
                Ok(Some(data)) => {
                    let state = app.state::<AppState>();
                    let mut guard = state.0.lock().unwrap();
                    *guard = data;
                }
                Ok(None) => {}
                Err(reason) => {
                    // 빈 상태로 시작하되, 다음 저장이 원본을 덮어쓰지 않도록 원본을 옆으로 옮겨 둔다
                    eprintln!("failed to load stored data: {reason}");
                    let (preserved_path, writes_blocked) = match storage::quarantine_data_file(&handle) {
                        Ok(path) => (Some(path.to_string_lossy().to_string()), false),
                        Err(e) => {
                            eprintln!("failed to preserve stored data: {e}");
                            (None, true)
                        }
                    };
                    let state = app.state::<LoadFailureState>();
                    let mut guard = state.0.lock().unwrap();
                    *guard = Some(LoadFailure {
                        reason,
                        preserved_path,
                        writes_blocked,
                    });
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
            take_load_failure,
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{FixedOffset, Utc};

    fn kst() -> FixedOffset {
        FixedOffset::east_opt(9 * 3600).unwrap()
    }

    fn ymd(s: &str) -> NaiveDate {
        parse_date(s).unwrap()
    }

    fn ts(tz: &impl TimeZone, s: &str) -> i64 {
        let naive = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap();
        tz.from_local_datetime(&naive).single().unwrap().timestamp()
    }

    fn task(created_at: i64, completed_at: Option<i64>) -> TodoItem {
        TodoItem {
            id: created_at as u64,
            text: "t".to_string(),
            completed: completed_at.is_some(),
            created_at,
            completed_at,
        }
    }

    #[test]
    fn day_start_uses_local_midnight() {
        // KST 2026-09-30 00:00 == UTC 2026-09-29 15:00
        assert_eq!(day_start(&kst(), ymd("2026-09-30")), ts(&Utc, "2026-09-29 15:00"));
    }

    #[test]
    fn early_morning_kst_task_counts_on_same_local_day() {
        let at = ts(&kst(), "2026-09-30 00:30");
        let data = AppData {
            tasks: vec![task(at, Some(at))],
            ..AppData::default()
        };

        let today = compute_daily_stats(&data, ymd("2026-09-30"), &kst());
        let yesterday = compute_daily_stats(&data, ymd("2026-09-29"), &kst());
        assert_eq!((today.tasks_created, today.tasks_completed), (1, 1));
        assert_eq!((yesterday.tasks_created, yesterday.tasks_completed), (0, 0));

        // 같은 시각이 UTC 기준으로는 전날이다 (이전 동작)
        let utc_prev_day = compute_daily_stats(&data, ymd("2026-09-29"), &Utc);
        assert_eq!(utc_prev_day.tasks_created, 1);
    }

    #[test]
    fn day_boundaries_are_half_open() {
        let midnight = ts(&kst(), "2026-10-01 00:00");
        let data = AppData {
            tasks: vec![task(midnight - 1, None), task(midnight, None)],
            ..AppData::default()
        };
        assert_eq!(compute_daily_stats(&data, ymd("2026-09-30"), &kst()).tasks_created, 1);
        assert_eq!(compute_daily_stats(&data, ymd("2026-10-01"), &kst()).tasks_created, 1);
    }

    #[test]
    fn date_range_is_inclusive_and_empty_when_reversed() {
        let days = date_range(ymd("2026-09-28"), ymd("2026-10-02"));
        assert_eq!(days.len(), 5);
        assert_eq!(days.first(), Some(&ymd("2026-09-28")));
        assert_eq!(days.last(), Some(&ymd("2026-10-02")));
        assert!(date_range(ymd("2026-10-02"), ymd("2026-09-28")).is_empty());
    }

    #[test]
    fn invalid_dates_are_rejected() {
        assert!(parse_date("").is_err());
        assert!(parse_date("2026-13-01").is_err());
        assert!(parse_date("2026-02-30").is_err());
        assert_eq!(parse_date(" 2026-02-28 ").unwrap(), NaiveDate::from_ymd_opt(2026, 2, 28).unwrap());
    }
}