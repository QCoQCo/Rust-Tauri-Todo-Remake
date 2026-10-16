// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// Learn more about Tauri commands at https://tauri.app/v1/guides/features/command
mod stopwatch;
mod storage;

use chrono::{Local, NaiveDate, TimeZone};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use stopwatch::{FocusSession, LapRecord, StopwatchState};
use tauri::{Manager, RunEvent};

// v2: 할 일 soft delete, 스탑워치 실행 상태, 집중 세션/Lap 기록 추가 (v1 데이터는 그대로 읽힘)
const DATA_VERSION: u32 = 2;

// 스탑워치 실행 중 체크포인트 저장 주기 (비정상 종료 시 최대 이만큼 기록이 빠짐)
const CHECKPOINT_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Clone, Serialize, Deserialize)]
struct TodoItem {
    id: u64,
    text: String,
    completed: bool,
    created_at: i64,
    completed_at: Option<i64>, // 완료 시각 (통계용)
    #[serde(default)]
    deleted_at: Option<i64>, // 삭제 시각. 목록에서는 숨기고 통계에는 남긴다
}

#[derive(Clone, Serialize, Deserialize)]
struct AppData {
    v: u32,
    tasks: Vec<TodoItem>,
    stopwatch: Option<StopwatchState>,
    #[serde(default)]
    focus_sessions: Vec<FocusSession>,
    #[serde(default)]
    lap_records: Vec<LapRecord>,
}

impl Default for AppData {
    fn default() -> Self {
        Self {
            v: DATA_VERSION,
            tasks: Vec::new(),
            stopwatch: None,
            focus_sessions: Vec::new(),
            lap_records: Vec::new(),
        }
    }
}

impl AppData {
    fn visible_tasks(&self) -> Vec<TodoItem> {
        self.tasks.iter().filter(|t| t.deleted_at.is_none()).cloned().collect()
    }

    fn stopwatch_view(&self) -> StopwatchState {
        self.stopwatch.clone().unwrap_or_default()
    }

    /// 스탑워치 상태를 꺼내 조작 (아직 없으면 새로 만듦)
    fn with_stopwatch<R>(&mut self, f: impl FnOnce(&mut StopwatchState) -> R) -> R {
        let sw = self.stopwatch.get_or_insert_with(StopwatchState::default);
        f(sw)
    }

    fn stop_stopwatch(&mut self, now_ms: u64) {
        if let Some(session) = self.with_stopwatch(|sw| sw.stop(now_ms)) {
            self.focus_sessions.push(session);
        }
    }

    /// 실행 중이었다면 그때까지의 집중 시간은 기록하고 초기화
    fn reset_stopwatch(&mut self, now_ms: u64) {
        self.stop_stopwatch(now_ms);
        self.stopwatch = None;
    }

    /// 정상 종료되지 못해 실행 상태로 남은 스탑워치를 마지막 체크포인트에서 멈춤. 변경이 있으면 true
    fn recover_interrupted_stopwatch(&mut self, now_ms: u64) -> bool {
        let Some(sw) = self.stopwatch.as_mut().filter(|sw| sw.is_running()) else {
            return false;
        };
        if let Some(session) = sw.recover_interrupted(now_ms) {
            self.focus_sessions.push(session);
        }
        true
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

// 백업 가져오기 후 화면을 다시 그리는 데 필요한 값
#[derive(Serialize)]
struct AppView {
    tasks: Vec<TodoItem>,
    stopwatch: StopwatchState,
}

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

/// 상태를 바꾸고 저장한다. 저장 순서가 뒤바뀌지 않도록 잠금을 쥔 채로 저장
fn update<R>(app: &tauri::AppHandle, f: impl FnOnce(&mut AppData) -> R) -> R {
    let state = app.state::<AppState>();
    let mut data = state.0.lock().unwrap();
    let out = f(&mut data);
    persist(app, &data);
    out
}

#[tauri::command]
fn take_load_failure(state: tauri::State<'_, LoadFailureState>) -> Option<LoadFailure> {
    state.0.lock().unwrap().take()
}

#[tauri::command]
fn get_tasks(state: tauri::State<'_, AppState>) -> Vec<TodoItem> {
    state.0.lock().unwrap().visible_tasks()
}

// 프론트 입력창의 maxlength와 맞춤
const MAX_TASK_TEXT_CHARS: usize = 120;

fn validate_task_text(text: &str) -> Result<String, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("task text is empty".to_string());
    }
    if text.chars().count() > MAX_TASK_TEXT_CHARS {
        return Err(format!("task text exceeds {MAX_TASK_TEXT_CHARS} characters"));
    }
    Ok(text.to_string())
}

// 같은 밀리초에 추가되거나 시계가 뒤로 가도 기존 id와 겹치지 않게 함 (삭제된 항목 포함)
fn next_task_id(tasks: &[TodoItem], now_ms: u64) -> u64 {
    let max_id = tasks.iter().map(|t| t.id).max().unwrap_or(0);
    now_ms.max(max_id.saturating_add(1))
}

#[tauri::command]
fn add_task(text: String, app: tauri::AppHandle) -> Result<Vec<TodoItem>, String> {
    let text = validate_task_text(&text)?;
    Ok(update(&app, |data| {
        let item = TodoItem {
            id: next_task_id(&data.tasks, now_millis()),
            text,
            completed: false,
            created_at: now_secs(),
            completed_at: None,
            deleted_at: None,
        };
        // 최신이 위로
        data.tasks.insert(0, item);
        data.visible_tasks()
    }))
}

#[tauri::command]
fn toggle_task(id: u64, app: tauri::AppHandle) -> Vec<TodoItem> {
    update(&app, |data| {
        if let Some(t) = data.tasks.iter_mut().find(|t| t.id == id && t.deleted_at.is_none()) {
            t.completed = !t.completed;
            t.completed_at = t.completed.then(now_secs);
        }
        data.visible_tasks()
    })
}

#[tauri::command]
fn delete_task(id: u64, app: tauri::AppHandle) -> Vec<TodoItem> {
    update(&app, |data| {
        if let Some(t) = data.tasks.iter_mut().find(|t| t.id == id && t.deleted_at.is_none()) {
            t.deleted_at = Some(now_secs());
        }
        data.visible_tasks()
    })
}

#[tauri::command]
fn get_stopwatch_state(state: tauri::State<'_, AppState>) -> StopwatchState {
    state.0.lock().unwrap().stopwatch_view()
}

#[tauri::command]
fn stopwatch_start(app: tauri::AppHandle) -> StopwatchState {
    update(&app, |data| {
        data.with_stopwatch(|sw| sw.start(now_millis()));
        data.stopwatch_view()
    })
}

#[tauri::command]
fn stopwatch_stop(app: tauri::AppHandle) -> StopwatchState {
    update(&app, |data| {
        data.stop_stopwatch(now_millis());
        data.stopwatch_view()
    })
}

#[tauri::command]
fn stopwatch_lap(app: tauri::AppHandle) -> StopwatchState {
    update(&app, |data| {
        if let Some(lap) = data.with_stopwatch(|sw| sw.lap(now_millis())) {
            data.lap_records.push(lap);
        }
        data.stopwatch_view()
    })
}

#[tauri::command]
fn stopwatch_delete_lap(index: usize, app: tauri::AppHandle) -> StopwatchState {
    update(&app, |data| {
        data.with_stopwatch(|sw| sw.delete_lap(index));
        data.stopwatch_view()
    })
}

#[tauri::command]
fn stopwatch_clear_laps(app: tauri::AppHandle) -> StopwatchState {
    update(&app, |data| {
        data.with_stopwatch(|sw| sw.clear_laps());
        data.stopwatch_view()
    })
}

#[tauri::command]
fn stopwatch_reset(app: tauri::AppHandle) -> StopwatchState {
    update(&app, |data| {
        data.reset_stopwatch(now_millis());
        data.stopwatch_view()
    })
}

#[tauri::command(rename_all = "snake_case")]
async fn export_data(
    state: tauri::State<'_, AppState>,
    file_path: String,
    password: String,
) -> Result<String, String> {
    let data = state.0.lock().unwrap().clone();
    let bytes = serde_json::to_vec(&data).map_err(|e| format!("serialize error: {e}"))?;

    let path = std::path::PathBuf::from(file_path);
    storage::export_backup(&path, &bytes, &password)?;
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command(rename_all = "snake_case")]
async fn backup_needs_password(file_path: String) -> Result<bool, String> {
    storage::backup_needs_password(std::path::Path::new(&file_path))
}

#[tauri::command(rename_all = "snake_case")]
async fn import_data(
    app: tauri::AppHandle,
    file_path: String,
    password: Option<String>,
) -> Result<AppView, String> {
    let path = std::path::PathBuf::from(file_path);

    let bytes = storage::import_backup(&app, &path, password.as_deref())?;
    let mut imported: AppData = serde_json::from_slice(&bytes).map_err(|e| format!("parse error: {e}"))?;
    imported.v = DATA_VERSION;
    // 실행 중에 내보낸 백업이면 내보낸 시점 근처(체크포인트)에서 멈춘 것으로 처리
    imported.recover_interrupted_stopwatch(now_millis());

    // 상태를 교체하고 즉시 저장
    Ok(update(&app, |data| {
        *data = imported;
        AppView {
            tasks: data.visible_tasks(),
            stopwatch: data.stopwatch_view(),
        }
    }))
}

// --- 통계 관련 구조체 ---
#[derive(Clone, Serialize)]
struct DailyStats {
    date: String, // YYYY-MM-DD
    tasks_completed: u32,
    tasks_created: u32,
    focus_time_ms: u64, // 그날 시작한 집중 세션 합계 (밀리초)
    lap_count: u32,
    avg_lap_time_ms: Option<u64>, // 평균 Lap 시간
    #[serde(skip)]
    lap_time_sum_ms: u64, // 기간 전체 평균 계산용
}

#[derive(Clone, Serialize)]
struct RangeStats {
    start_date: String, // YYYY-MM-DD
    end_date: String,
    total_tasks_completed: u32,
    total_tasks_created: u32,
    total_focus_time_ms: u64,
    total_lap_count: u32,
    avg_lap_time_ms: Option<u64>, // 기간 전체 Lap의 평균 (일별 평균의 평균이 아님)
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

fn average(sum: u64, count: u32) -> Option<u64> {
    (count > 0).then(|| sum / count as u64)
}

fn compute_daily_stats<Tz: TimeZone>(data: &AppData, date: NaiveDate, tz: &Tz) -> DailyStats {
    // 하루 길이를 86400초로 가정하지 않고 다음 날 시작 시각까지로 계산 (DST 대응)
    let start_ts = day_start(tz, date);
    let end_ts = date.succ_opt().map_or(i64::MAX, |next| day_start(tz, next));
    let in_day = |ts: i64| ts >= start_ts && ts < end_ts;
    let in_day_ms = |ms: u64| in_day((ms / 1000) as i64);

    // 삭제된 할 일도 통계에는 포함
    let tasks_completed = data
        .tasks
        .iter()
        .filter(|t| t.completed && t.completed_at.is_some_and(in_day))
        .count() as u32;

    let tasks_created = data.tasks.iter().filter(|t| in_day(t.created_at)).count() as u32;

    // 자정을 넘긴 세션은 시작한 날로 집계
    let focus_time_ms = data
        .focus_sessions
        .iter()
        .filter(|s| in_day_ms(s.started_at_ms))
        .map(|s| s.duration_ms)
        .sum();

    let (lap_count, lap_time_sum_ms) = data
        .lap_records
        .iter()
        .filter(|l| in_day_ms(l.at_ms))
        .fold((0u32, 0u64), |(count, sum), l| (count + 1, sum + l.split_ms));

    DailyStats {
        date: date.format("%Y-%m-%d").to_string(),
        tasks_completed,
        tasks_created,
        focus_time_ms,
        lap_count,
        avg_lap_time_ms: average(lap_time_sum_ms, lap_count),
        lap_time_sum_ms,
    }
}

fn compute_range_stats<Tz: TimeZone>(data: &AppData, start: NaiveDate, end: NaiveDate, tz: &Tz) -> RangeStats {
    let daily_stats: Vec<DailyStats> = date_range(start, end)
        .into_iter()
        .map(|date| compute_daily_stats(data, date, tz))
        .collect();
    let total_lap_count = daily_stats.iter().map(|d| d.lap_count).sum();
    let lap_time_sum_ms = daily_stats.iter().map(|d| d.lap_time_sum_ms).sum();

    RangeStats {
        start_date: start.format("%Y-%m-%d").to_string(),
        end_date: end.format("%Y-%m-%d").to_string(),
        total_tasks_completed: daily_stats.iter().map(|d| d.tasks_completed).sum(),
        total_tasks_created: daily_stats.iter().map(|d| d.tasks_created).sum(),
        total_focus_time_ms: daily_stats.iter().map(|d| d.focus_time_ms).sum(),
        total_lap_count,
        avg_lap_time_ms: average(lap_time_sum_ms, total_lap_count),
        daily_stats,
    }
}

#[tauri::command(rename_all = "snake_case")]
fn get_range_stats(
    start_date: String,
    end_date: String,
    state: tauri::State<'_, AppState>,
) -> Result<RangeStats, String> {
    let (start, end) = (parse_date(&start_date)?, parse_date(&end_date)?);
    let data = state.0.lock().unwrap();
    Ok(compute_range_stats(&data, start, end, &Local))
}

#[tauri::command(rename_all = "snake_case")]
async fn export_stats_csv(
    start_date: String,
    end_date: String,
    file_path: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    let (start, end) = (parse_date(&start_date)?, parse_date(&end_date)?);
    let stats = compute_range_stats(&state.0.lock().unwrap(), start, end, &Local);
    let mut csv = String::from("날짜,완료된 할 일,생성된 할 일,집중 시간(분),Lap 수,평균 Lap 시간(초)\n");

    for day in stats.daily_stats {
        let focus_min = day.focus_time_ms / 60000;
        let avg_lap_sec = day.avg_lap_time_ms.map(|ms| ms / 1000).unwrap_or(0);
        csv.push_str(&format!(
            "{},{},{},{},{},{}\n",
            day.date,
            day.tasks_completed,
            day.tasks_created,
            focus_min,
            day.lap_count,
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

/// 스탑워치가 도는 동안 주기적으로 체크포인트를 저장 (비정상 종료 대비)
fn spawn_checkpoint_thread(app: tauri::AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(CHECKPOINT_INTERVAL);
        let state = app.state::<AppState>();
        let mut data = state.0.lock().unwrap();
        let now = now_millis();
        if data.stopwatch.as_mut().is_some_and(|sw| sw.checkpoint(now)) {
            persist(&app, &data);
        }
    });
}

fn main() {
    let app = tauri::Builder::default()
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
                Ok(Some(mut data)) => {
                    data.v = DATA_VERSION;
                    // 지난번에 비정상 종료됐다면 실행 중이던 스탑워치를 마지막 체크포인트에서 멈춤
                    let recovered = data.recover_interrupted_stopwatch(now_millis());
                    if recovered {
                        persist(&handle, &data);
                    }
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
            spawn_checkpoint_thread(handle);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            take_load_failure,
            get_tasks,
            add_task,
            toggle_task,
            delete_task,
            get_stopwatch_state,
            stopwatch_start,
            stopwatch_stop,
            stopwatch_lap,
            stopwatch_delete_lap,
            stopwatch_clear_laps,
            stopwatch_reset,
            export_data,
            backup_needs_password,
            import_data,
            get_range_stats,
            export_stats_csv
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app, event| {
        // 앱 종료 = 스탑워치 일시정지 (종료 시점까지 기록)
        if let RunEvent::Exit = event {
            let state = app.state::<AppState>();
            let mut data = state.0.lock().unwrap();
            if data.stopwatch.as_ref().is_some_and(StopwatchState::is_running) {
                data.stop_stopwatch(now_millis());
                persist(app, &data);
            }
        }
    });
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
            deleted_at: None,
        }
    }

    fn ms(tz: &impl TimeZone, s: &str) -> u64 {
        ts(tz, s) as u64 * 1000
    }

    #[test]
    fn deleted_tasks_are_hidden_but_still_counted() {
        let at = ts(&kst(), "2026-10-01 10:00");
        let data = AppData {
            tasks: vec![TodoItem { deleted_at: Some(at + 60), ..task(at, Some(at)) }, task(at + 1, None)],
            ..AppData::default()
        };
        assert_eq!(data.visible_tasks().len(), 1);
        let stats = compute_daily_stats(&data, ymd("2026-10-01"), &kst());
        assert_eq!((stats.tasks_created, stats.tasks_completed), (2, 1));
    }

    #[test]
    fn focus_and_laps_are_counted_on_their_own_day() {
        let data = AppData {
            focus_sessions: vec![
                FocusSession { started_at_ms: ms(&kst(), "2026-10-01 00:10"), duration_ms: 60_000 },
                FocusSession { started_at_ms: ms(&kst(), "2026-10-01 23:50"), duration_ms: 30 * 60_000 }, // 자정 넘김
                FocusSession { started_at_ms: ms(&kst(), "2026-10-02 09:00"), duration_ms: 5_000 },
            ],
            lap_records: vec![
                LapRecord { at_ms: ms(&kst(), "2026-10-01 00:11"), split_ms: 1_000 },
                LapRecord { at_ms: ms(&kst(), "2026-10-01 00:12"), split_ms: 2_000 },
                LapRecord { at_ms: ms(&kst(), "2026-10-02 09:00"), split_ms: 6_000 },
            ],
            ..AppData::default()
        };

        let day1 = compute_daily_stats(&data, ymd("2026-10-01"), &kst());
        assert_eq!(day1.focus_time_ms, 31 * 60_000);
        assert_eq!((day1.lap_count, day1.avg_lap_time_ms), (2, Some(1_500)));

        let day2 = compute_daily_stats(&data, ymd("2026-10-02"), &kst());
        assert_eq!(day2.focus_time_ms, 5_000);
        assert_eq!((day2.lap_count, day2.avg_lap_time_ms), (1, Some(6_000)));

        let empty = compute_daily_stats(&data, ymd("2026-10-03"), &kst());
        assert_eq!((empty.focus_time_ms, empty.lap_count, empty.avg_lap_time_ms), (0, 0, None));

        let range = compute_range_stats(&data, ymd("2026-10-01"), ymd("2026-10-03"), &kst());
        assert_eq!(range.daily_stats.len(), 3);
        assert_eq!(range.total_focus_time_ms, 31 * 60_000 + 5_000);
        assert_eq!(range.total_lap_count, 3);
        // 일별 평균의 평균(3_750)이 아니라 전체 Lap의 평균
        assert_eq!(range.avg_lap_time_ms, Some(3_000));
    }

    #[test]
    fn v1_data_loads_with_defaults() {
        let v1 = r#"{"v":1,"tasks":[{"id":1,"text":"a","completed":false,"created_at":0,"completed_at":null}],
                     "stopwatch":{"elapsed_ms":5000,"lap_totals_ms":[1000]}}"#;
        let data: AppData = serde_json::from_str(v1).unwrap();
        assert_eq!(data.visible_tasks().len(), 1);
        assert!(data.focus_sessions.is_empty() && data.lap_records.is_empty());
        assert_eq!(data.stopwatch_view().elapsed_ms, 5000);
        assert!(!data.stopwatch_view().is_running());
    }

    #[test]
    fn interrupted_stopwatch_is_recovered_into_a_session() {
        let mut data = AppData::default();
        data.with_stopwatch(|sw| {
            sw.start(1_000);
            sw.checkpoint(31_000);
        });
        assert!(data.recover_interrupted_stopwatch(500_000));
        assert_eq!(data.focus_sessions, vec![FocusSession { started_at_ms: 1_000, duration_ms: 30_000 }]);
        assert!(!data.recover_interrupted_stopwatch(600_000), "nothing left to recover");
    }

    #[test]
    fn reset_records_running_time_before_clearing() {
        let mut data = AppData::default();
        data.with_stopwatch(|sw| sw.start(0));
        data.reset_stopwatch(10_000);
        assert_eq!(data.focus_sessions, vec![FocusSession { started_at_ms: 0, duration_ms: 10_000 }]);
        assert_eq!(data.stopwatch_view(), StopwatchState::default());
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
    fn task_text_is_trimmed_and_validated() {
        assert_eq!(validate_task_text("  장보기  ").unwrap(), "장보기");
        assert!(validate_task_text("").is_err());
        assert!(validate_task_text(" \t\n ").is_err());
        assert!(validate_task_text(&"가".repeat(MAX_TASK_TEXT_CHARS)).is_ok());
        assert!(validate_task_text(&"가".repeat(MAX_TASK_TEXT_CHARS + 1)).is_err());
    }

    #[test]
    fn task_ids_never_collide() {
        assert_eq!(next_task_id(&[], 1_000), 1_000);
        let tasks = vec![TodoItem { id: 1_000, ..task(0, None) }];
        // 같은 밀리초
        assert_eq!(next_task_id(&tasks, 1_000), 1_001);
        // 시계가 뒤로 간 경우 / 가져온 데이터의 id가 더 큰 경우
        assert_eq!(next_task_id(&tasks, 500), 1_001);
        assert_eq!(next_task_id(&tasks, 2_000), 2_000);
    }

    #[test]
    fn invalid_dates_are_rejected() {
        assert!(parse_date("").is_err());
        assert!(parse_date("2026-13-01").is_err());
        assert!(parse_date("2026-02-30").is_err());
        assert_eq!(parse_date(" 2026-02-28 ").unwrap(), NaiveDate::from_ymd_opt(2026, 2, 28).unwrap());
    }
}