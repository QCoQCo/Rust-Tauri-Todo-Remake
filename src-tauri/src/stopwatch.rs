use serde::{Deserialize, Serialize};

/// 화면에 보이는 스탑워치 상태. 실행 중이면 `running_since_ms`부터 지금까지가 더해진다.
#[derive(Clone, Serialize, Deserialize, Default, Debug, PartialEq)]
pub struct StopwatchState {
    /// 현재 실행 구간을 제외하고 누적된 시간
    pub elapsed_ms: u64,
    pub lap_totals_ms: Vec<u64>,
    /// 실행 중이면 이번 실행 구간이 시작된 시각 (epoch ms)
    #[serde(default)]
    pub running_since_ms: Option<u64>,
    /// 실행 중 마지막으로 살아 있음을 확인한 시각. 비정상 종료 시 여기까지만 기록한다
    #[serde(default)]
    pub checkpoint_ms: Option<u64>,
}

/// 시작~일시정지 한 구간 (통계용)
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct FocusSession {
    pub started_at_ms: u64,
    pub duration_ms: u64,
}

/// Lap 한 번 (통계용). 화면의 Lap 목록을 지워도 기록은 남는다
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct LapRecord {
    pub at_ms: u64,
    pub split_ms: u64,
}

impl StopwatchState {
    pub fn is_running(&self) -> bool {
        self.running_since_ms.is_some()
    }

    pub fn total_ms(&self, now_ms: u64) -> u64 {
        let running = self.running_since_ms.map_or(0, |since| now_ms.saturating_sub(since));
        self.elapsed_ms + running
    }

    pub fn start(&mut self, now_ms: u64) {
        if !self.is_running() {
            self.running_since_ms = Some(now_ms);
            self.checkpoint_ms = Some(now_ms);
        }
    }

    /// 실행 중이었다면 그 구간을 세션으로 돌려준다 (0ms 구간은 버림)
    pub fn stop(&mut self, now_ms: u64) -> Option<FocusSession> {
        let since = self.running_since_ms.take()?;
        self.checkpoint_ms = None;
        let duration_ms = now_ms.saturating_sub(since);
        self.elapsed_ms += duration_ms;
        (duration_ms > 0).then_some(FocusSession {
            started_at_ms: since,
            duration_ms,
        })
    }

    /// 직전 Lap 이후 시간이 흘렀을 때만 Lap을 남긴다
    pub fn lap(&mut self, now_ms: u64) -> Option<LapRecord> {
        let total = self.total_ms(now_ms);
        let last = self.lap_totals_ms.last().copied().unwrap_or(0);
        if total <= last {
            return None;
        }
        self.lap_totals_ms.push(total);
        Some(LapRecord {
            at_ms: now_ms,
            split_ms: total - last,
        })
    }

    pub fn delete_lap(&mut self, index: usize) {
        if index < self.lap_totals_ms.len() {
            self.lap_totals_ms.remove(index);
        }
    }

    pub fn clear_laps(&mut self) {
        self.lap_totals_ms.clear();
    }

    /// 실행 중이면 체크포인트를 갱신하고 true
    pub fn checkpoint(&mut self, now_ms: u64) -> bool {
        if self.is_running() {
            self.checkpoint_ms = Some(now_ms);
            true
        } else {
            false
        }
    }

    /// 앱이 정상 종료되지 못해 실행 상태로 남아 있으면, 마지막 체크포인트 시각에 일시정지한 것으로 처리
    pub fn recover_interrupted(&mut self, now_ms: u64) -> Option<FocusSession> {
        let since = self.running_since_ms?;
        let end = self.checkpoint_ms.unwrap_or(since).clamp(since, now_ms.max(since));
        self.stop(end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_stop_accumulates_and_records_session() {
        let mut sw = StopwatchState::default();
        sw.start(1_000);
        assert!(sw.is_running());
        assert_eq!(sw.total_ms(1_500), 500);

        let session = sw.stop(3_000).unwrap();
        assert_eq!(session, FocusSession { started_at_ms: 1_000, duration_ms: 2_000 });
        assert_eq!(sw.elapsed_ms, 2_000);
        assert!(!sw.is_running());

        // 다시 시작해도 누적
        sw.start(10_000);
        sw.stop(10_500);
        assert_eq!(sw.elapsed_ms, 2_500);
    }

    #[test]
    fn start_twice_keeps_original_start_and_stop_when_idle_is_noop() {
        let mut sw = StopwatchState::default();
        assert!(sw.stop(5).is_none());
        sw.start(100);
        sw.start(200);
        assert_eq!(sw.running_since_ms, Some(100));
        assert!(sw.stop(100).is_none(), "0ms session is not recorded");
    }

    #[test]
    fn lap_records_split_and_skips_duplicates() {
        let mut sw = StopwatchState::default();
        assert!(sw.lap(0).is_none(), "nothing elapsed yet");
        sw.start(0);
        assert_eq!(sw.lap(1_000).unwrap().split_ms, 1_000);
        assert_eq!(sw.lap(2_500).unwrap().split_ms, 1_500);
        sw.stop(3_000);
        assert_eq!(sw.lap(9_000).unwrap().split_ms, 500, "paused time is not counted");
        assert!(sw.lap(9_999).is_none(), "no time passed since last lap while paused");
        assert_eq!(sw.lap_totals_ms, vec![1_000, 2_500, 3_000]);
    }

    #[test]
    fn delete_and_clear_laps_only_touch_display() {
        let mut sw = StopwatchState { lap_totals_ms: vec![1, 2, 3], ..Default::default() };
        sw.delete_lap(1);
        sw.delete_lap(99);
        assert_eq!(sw.lap_totals_ms, vec![1, 3]);
        sw.clear_laps();
        assert!(sw.lap_totals_ms.is_empty());
    }

    #[test]
    fn interrupted_run_is_closed_at_last_checkpoint() {
        let mut sw = StopwatchState::default();
        sw.start(1_000);
        assert!(sw.checkpoint(31_000));
        // 앱이 죽고 한참 뒤 다시 켬
        let session = sw.recover_interrupted(999_000).unwrap();
        assert_eq!(session.duration_ms, 30_000);
        assert_eq!(sw.elapsed_ms, 30_000);
        assert!(!sw.is_running());
        assert!(sw.recover_interrupted(1_000_000).is_none());
    }

    #[test]
    fn interrupted_run_without_checkpoint_records_nothing() {
        // 이전 버전 데이터처럼 checkpoint 없이 실행 상태만 있는 경우
        let mut sw = StopwatchState { running_since_ms: Some(1_000), ..Default::default() };
        assert!(sw.recover_interrupted(50_000).is_none());
        assert!(!sw.is_running());
        assert_eq!(sw.elapsed_ms, 0);
    }

    #[test]
    fn old_format_deserializes() {
        let sw: StopwatchState = serde_json::from_str(r#"{"elapsed_ms":42,"lap_totals_ms":[10,20]}"#).unwrap();
        assert_eq!(sw.elapsed_ms, 42);
        assert!(!sw.is_running());
    }
}
