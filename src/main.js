const tauriInvoke = globalThis?.__TAURI__?.tauri?.invoke;
const tauriDialog = globalThis?.__TAURI__?.dialog;

function $(id) {
    const el = document.getElementById(id);
    if (!el) throw new Error(`Missing element #${id}`);
    return el;
}

function formatClock(date) {
    return new Intl.DateTimeFormat('ko-KR', {
        weekday: 'short',
        hour: '2-digit',
        minute: '2-digit',
        second: '2-digit',
        hour12: false,
    }).format(date);
}

function startClock() {
    const clockEl = $('clock');
    const tick = () => {
        clockEl.textContent = formatClock(new Date());
    };
    tick();
    setInterval(tick, 1000);
}

function formatStopwatch(ms) {
    const totalCentis = Math.floor(ms / 10);
    const centis = totalCentis % 100;
    const totalSeconds = Math.floor(totalCentis / 100);
    const seconds = totalSeconds % 60;
    const minutes = Math.floor(totalSeconds / 60);
    return `${String(minutes).padStart(2, '0')}:${String(seconds).padStart(2, '0')}.${String(centis).padStart(2, '0')}`;
}

const STOPWATCH_STORAGE_KEY = 'todo_app_stopwatch_state_v1';

// 백엔드 StopwatchState와 같은 모양으로 정리 (이전 형식/깨진 값도 허용)
function normalizeStopwatch(raw) {
    const s = raw && typeof raw === 'object' ? raw : {};
    const nonNegative = (v) => {
        const n = Number(v);
        return Number.isFinite(n) && n >= 0 ? n : 0;
    };
    return {
        elapsed_ms: nonNegative(s.elapsed_ms),
        lap_totals_ms: Array.isArray(s.lap_totals_ms)
            ? s.lap_totals_ms.map(Number).filter((n) => Number.isFinite(n) && n >= 0)
            : [],
        running_since_ms: s.running_since_ms == null ? null : nonNegative(s.running_since_ms),
    };
}

function stopwatchTotal(state, now = Date.now()) {
    const running = state.running_since_ms == null ? 0 : Math.max(0, now - state.running_since_ms);
    return state.elapsed_ms + running;
}

function loadLocalStopwatch() {
    try {
        const raw = localStorage.getItem(STOPWATCH_STORAGE_KEY);
        return normalizeStopwatch(raw ? JSON.parse(raw) : null);
    } catch {
        return normalizeStopwatch(null);
    }
}

function saveLocalStopwatch(state) {
    try {
        localStorage.setItem(STOPWATCH_STORAGE_KEY, JSON.stringify(state));
    } catch {
        // ignore
    }
}

// Tauri가 없을 때 백엔드 스탑워치 커맨드를 흉내 냄 (통계용 기록은 남기지 않음)
function runLocalStopwatchCommand(command, args = {}) {
    let s = loadLocalStopwatch();
    const now = Date.now();
    const running = s.running_since_ms != null;
    switch (command) {
        case 'stopwatch_start':
            if (!running) s.running_since_ms = now;
            break;
        case 'stopwatch_stop':
            if (running) s = { ...s, elapsed_ms: stopwatchTotal(s, now), running_since_ms: null };
            break;
        case 'stopwatch_lap': {
            const total = stopwatchTotal(s, now);
            const last = s.lap_totals_ms.at(-1) ?? 0;
            if (total > last) s.lap_totals_ms.push(total);
            break;
        }
        case 'stopwatch_delete_lap':
            s.lap_totals_ms = s.lap_totals_ms.filter((_, idx) => idx !== args.index);
            break;
        case 'stopwatch_clear_laps':
            s.lap_totals_ms = [];
            break;
        case 'stopwatch_reset':
            s = normalizeStopwatch(null);
            break;
        default: // get_stopwatch_state
            return s;
    }
    saveLocalStopwatch(s);
    return s;
}

async function setupStopwatch() {
    const toggleBtn = $('toggle-stopwatch');
    const panel = $('stopwatch-panel');
    const timeEl = $('stopwatch-time');
    const statusEl = $('stopwatch-status');
    const startBtn = $('stopwatch-start');
    const lapBtn = $('stopwatch-lap');
    const resetBtn = $('stopwatch-reset');
    const lapsList = $('stopwatch-laps');
    const lapsEmpty = $('stopwatch-laps-empty');
    const lapsClearBtn = $('stopwatch-laps-clear');

    // 상태의 주인은 백엔드(또는 localStorage). 화면은 받은 상태를 그리기만 함
    let state = normalizeStopwatch(null);
    let ready = false; // 처음 불러오기 전에는 조작 막기
    let timer = null;

    const isRunning = () => state.running_since_ms != null;

    const render = () => {
        timeEl.textContent = formatStopwatch(stopwatchTotal(state));
    };

    const renderControls = () => {
        const running = isRunning();
        const total = stopwatchTotal(state);
        startBtn.textContent = running ? '일시정지' : '시작';
        statusEl.textContent = running ? 'running' : total > 0 ? 'paused' : 'ready';
        startBtn.disabled = !ready;
        resetBtn.disabled = !ready;
        lapBtn.disabled = !ready || (!running && total <= 0);
    };

    const renderLaps = () => {
        lapsList.innerHTML = '';
        const lapTotals = state.lap_totals_ms;
        if (lapTotals.length === 0) {
            lapsEmpty.hidden = false;
            lapsClearBtn.disabled = true;
            return;
        }
        lapsEmpty.hidden = true;
        lapsClearBtn.disabled = false;

        // 최신 lap이 위로 보이도록 reverse render
        for (let i = lapTotals.length - 1; i >= 0; i -= 1) {
            const totalMs = lapTotals[i];
            const prevTotal = i === 0 ? 0 : lapTotals[i - 1];
            const splitMs = totalMs - prevTotal;
            const lapNo = i + 1;

            const li = document.createElement('li');
            li.className = 'lap-item';

            const left = document.createElement('div');
            const label = document.createElement('div');
            label.className = 'lap-item__label';
            label.textContent = `Lap ${lapNo}`;

            const total = document.createElement('div');
            total.className = 'lap-item__time';
            total.textContent = formatStopwatch(totalMs);

            left.appendChild(label);
            left.appendChild(total);

            const split = document.createElement('div');
            split.className = 'lap-item__split';
            split.textContent = `+${formatStopwatch(splitMs)}`;

            const del = document.createElement('button');
            del.type = 'button';
            del.className = 'lap-item__delete';
            del.textContent = '삭제';
            del.setAttribute('data-lap-index', String(i));

            li.appendChild(left);
            li.appendChild(split);
            li.appendChild(del);
            lapsList.appendChild(li);
        }
    };

    const applyState = (next) => {
        state = normalizeStopwatch(next);
        if (isRunning() && !timer) timer = setInterval(render, 50);
        if (!isRunning() && timer) {
            clearInterval(timer);
            timer = null;
        }
        render();
        renderLaps();
        renderControls();
    };

    const send = async (command, args = {}) => {
        try {
            const next = await invokeOrFallback(command, args, async () =>
                runLocalStopwatchCommand(command, args),
            );
            applyState(next);
        } catch (e) {
            console.error(e);
            window.alert(`스탑워치 오류: ${e}`);
        }
    };

    toggleBtn.addEventListener('click', () => {
        const isOpen = !panel.hidden;
        panel.hidden = isOpen;
        toggleBtn.setAttribute('aria-expanded', String(!isOpen));
        if (isOpen && isRunning()) send('stopwatch_stop');
    });

    startBtn.addEventListener('click', () => send(isRunning() ? 'stopwatch_stop' : 'stopwatch_start'));
    lapBtn.addEventListener('click', () => send('stopwatch_lap'));
    resetBtn.addEventListener('click', () => send('stopwatch_reset'));
    lapsClearBtn.addEventListener('click', () => send('stopwatch_clear_laps'));

    lapsList.addEventListener('click', (e) => {
        const btn = e.target?.closest?.('[data-lap-index]');
        const idx = Number(btn?.getAttribute?.('data-lap-index'));
        if (!btn || !Number.isInteger(idx)) return;
        send('stopwatch_delete_lap', { index: idx });
    });

    // Tauri 앱은 종료 시 백엔드가 일시정지 처리. 브라우저(localStorage)에서는 페이지를 떠날 때 멈춤
    if (typeof tauriInvoke !== 'function') {
        window.addEventListener('pagehide', () => {
            if (isRunning()) runLocalStopwatchCommand('stopwatch_stop');
        });
    }

    renderControls();
    await send('get_stopwatch_state');
    ready = true;
    renderControls();

    return { applyState };
}

// 백업 가져오기 후 스탑워치 화면 갱신용 (setupStopwatch가 끝나면 채워짐)
let stopwatchUi = null;

// --- Todo (Tauri 우선, 없으면 localStorage fallback) ---
const STORAGE_KEY = 'todo_app_tasks_v1';
let currentTasks = [];
let currentFilter = 'all'; // 'all' | 'inprogress' | 'done'

function loadLocalTasks() {
    try {
        const raw = localStorage.getItem(STORAGE_KEY);
        const parsed = raw ? JSON.parse(raw) : [];
        return Array.isArray(parsed) ? parsed : [];
    } catch {
        return [];
    }
}

function saveLocalTasks(tasks) {
    try {
        localStorage.setItem(STORAGE_KEY, JSON.stringify(tasks));
    } catch {
        // ignore
    }
}

async function invokeOrFallback(command, payload, fallbackFn) {
    if (typeof tauriInvoke === 'function') {
        return await tauriInvoke(command, payload);
    }
    return await fallbackFn();
}

function renderTasks(tasks) {
    const list = $('task-list');
    const empty = $('empty-state');
    const totalEl = document.getElementById('status-total');
    const doneEl = document.getElementById('status-done');
    const inProgressEl = document.getElementById('status-inprogress');

    const safeTasks = Array.isArray(tasks) ? tasks : [];
    currentTasks = safeTasks;
    const totalCount = safeTasks.length;
    const doneCount = safeTasks.reduce((acc, t) => acc + (t?.completed ? 1 : 0), 0);
    const inProgressCount = Math.max(0, totalCount - doneCount);
    if (totalEl) totalEl.textContent = String(totalCount);
    if (doneEl) doneEl.textContent = String(doneCount);
    if (inProgressEl) inProgressEl.textContent = String(inProgressCount);

    list.innerHTML = '';

    if (safeTasks.length === 0) {
        empty.textContent = '아직 할 일이 없어요.';
        empty.hidden = false;
        return;
    }

    // 완료 항목은 자동으로 아래로 (원래 순서는 유지)
    const activeTasks = safeTasks.filter((t) => !t?.completed);
    const doneTasks = safeTasks.filter((t) => Boolean(t?.completed));
    const sortedTasks = [...activeTasks, ...doneTasks];

    const filteredTasks =
        currentFilter === 'done'
            ? doneTasks
            : currentFilter === 'inprogress'
              ? activeTasks
              : sortedTasks;

    if (filteredTasks.length === 0) {
        empty.textContent = '해당 조건의 할 일이 없어요.';
        empty.hidden = false;
        return;
    }

    empty.hidden = true;

    filteredTasks.forEach((t) => {
        const li = document.createElement('li');
        li.className = 'todo-item' + (t.completed ? ' --done' : '');

        const cb = document.createElement('input');
        cb.type = 'checkbox';
        cb.className = 'checkbox';
        cb.checked = Boolean(t.completed);
        cb.setAttribute('aria-label', '완료 표시');

        const text = document.createElement('div');
        text.className = 'todo-item__text';
        text.textContent = t.text ?? '';

        const del = document.createElement('button');
        del.type = 'button';
        del.className = 'btn todo-item__delete';
        del.textContent = '삭제';

        cb.addEventListener('change', async () => {
            const updated = await invokeOrFallback('toggle_task', { id: t.id }, async () => {
                const local = loadLocalTasks().map((x) => {
                    if (x.id === t.id) {
                        const newCompleted = !x.completed;
                        return {
                            ...x,
                            completed: newCompleted,
                            completed_at: newCompleted ? Math.floor(Date.now() / 1000) : null,
                        };
                    }
                    return x;
                });
                saveLocalTasks(local);
                return local;
            });
            renderTasks(updated);
        });

        del.addEventListener('click', async () => {
            const updated = await invokeOrFallback('delete_task', { id: t.id }, async () => {
                const local = loadLocalTasks().filter((x) => x.id !== t.id);
                saveLocalTasks(local);
                return local;
            });
            renderTasks(updated);
        });

        li.appendChild(cb);
        li.appendChild(text);
        li.appendChild(del);
        list.appendChild(li);
    });
}

// 백엔드 storage::MIN_BACKUP_PASSWORD_CHARS와 같은 값
const MIN_BACKUP_PASSWORD_CHARS = 8;

// 백업 비밀번호 입력 창. 확인하면 비밀번호, 취소하면 null
function askBackupPassword({ title, description, confirm = false, error = '' }) {
    const dialog = $('password-dialog');
    const form = $('password-form');
    const input = $('password-input');
    const confirmInput = $('password-confirm');
    const errorEl = $('password-error');
    const cancelBtn = $('password-cancel');

    $('password-title').textContent = title;
    $('password-desc').textContent = description;
    input.value = '';
    input.autocomplete = confirm ? 'new-password' : 'current-password';
    confirmInput.value = '';
    confirmInput.hidden = !confirm;
    const showError = (message) => {
        errorEl.textContent = message;
        errorEl.hidden = !message;
    };
    showError(error);

    return new Promise((resolve) => {
        const finish = (value) => {
            form.removeEventListener('submit', onSubmit);
            dialog.removeEventListener('cancel', onCancel);
            cancelBtn.removeEventListener('click', onCancel);
            input.value = '';
            confirmInput.value = '';
            dialog.close();
            resolve(value);
        };
        const onSubmit = (e) => {
            e.preventDefault();
            const password = input.value;
            if ([...password].length < MIN_BACKUP_PASSWORD_CHARS) {
                showError(`비밀번호는 ${MIN_BACKUP_PASSWORD_CHARS}자 이상이어야 합니다.`);
            } else if (confirm && password !== confirmInput.value) {
                showError('비밀번호 확인이 일치하지 않습니다.');
            } else {
                finish(password);
            }
        };
        // 취소 버튼과 Esc 키
        const onCancel = (e) => {
            e.preventDefault();
            finish(null);
        };
        form.addEventListener('submit', onSubmit);
        dialog.addEventListener('cancel', onCancel);
        cancelBtn.addEventListener('click', onCancel);
        dialog.showModal();
        input.focus();
    });
}

async function exportData() {
    if (typeof tauriInvoke !== 'function' || !tauriDialog) {
        window.alert('Tauri 환경에서만 백업 내보내기가 가능합니다.');
        return;
    }
    try {
        const password = await askBackupPassword({
            title: '백업 비밀번호 설정',
            description:
                '이 비밀번호로 백업을 암호화합니다. 다른 PC에서도 이 비밀번호로 복원할 수 있으며, 잊어버리면 복원할 수 없습니다.',
            confirm: true,
        });
        if (password == null) return; // 사용자가 취소

        const timestamp = Math.floor(Date.now() / 1000);
        const defaultName = `todo_backup_${timestamp}.json`;
        const filePath = await tauriDialog.save({
            defaultPath: defaultName,
            filters: [{ name: 'Backup', extensions: ['json'] }],
        });
        if (!filePath || (Array.isArray(filePath) && filePath.length === 0)) {
            return; // 사용자가 취소
        }
        // Tauri v1은 배열을 반환할 수 있으므로 첫 번째 요소 사용
        const path = Array.isArray(filePath) ? filePath[0] : filePath;
        const savedPath = await tauriInvoke('export_data', { file_path: path, password });
        window.alert(`백업이 저장되었습니다:\n${savedPath}`);
    } catch (e) {
        if (String(e).includes('cancelled') || String(e).includes('user cancelled')) {
            return; // 사용자가 취소
        }
        window.alert(`백업 내보내기 실패: ${e}`);
        console.error(e);
    }
}

async function importData() {
    if (typeof tauriInvoke !== 'function' || !tauriDialog) {
        window.alert('Tauri 환경에서만 백업 가져오기가 가능합니다.');
        return;
    }
    if (!window.confirm('현재 데이터가 백업 파일로 대체됩니다. 계속하시겠습니까?')) {
        return;
    }
    try {
        const filePath = await tauriDialog.open({
            filters: [{ name: 'Backup', extensions: ['json'] }],
        });
        if (!filePath || (Array.isArray(filePath) && filePath.length === 0)) {
            return; // 사용자가 취소
        }
        // Tauri v1은 배열을 반환할 수 있으므로 첫 번째 요소 사용
        const path = Array.isArray(filePath) ? filePath[0] : filePath;

        // 예전(v1) 백업은 이 기기의 키로 열리므로 비밀번호를 묻지 않음
        const needsPassword = await tauriInvoke('backup_needs_password', { file_path: path });
        let error = '';
        let imported;
        for (;;) {
            let password = null;
            if (needsPassword) {
                password = await askBackupPassword({
                    title: '백업 비밀번호',
                    description: '백업을 만들 때 설정한 비밀번호를 입력하세요.',
                    error,
                });
                if (password == null) return; // 사용자가 취소
            }
            try {
                // 백엔드가 상태 교체와 저장을 끝낸 뒤 화면에 필요한 값만 돌려줌
                imported = await tauriInvoke('import_data', { file_path: path, password });
                break;
            } catch (e) {
                // 비밀번호가 틀리면 다시 묻기
                if (!needsPassword || !String(e).includes('wrong password')) throw e;
                error = '비밀번호가 틀렸거나 백업 파일이 손상되었습니다.';
            }
        }
        renderTasks(imported.tasks);
        stopwatchUi?.applyState(imported.stopwatch);
        window.alert('백업이 성공적으로 가져와졌습니다.');
    } catch (e) {
        if (String(e).includes('cancelled') || String(e).includes('user cancelled')) {
            return; // 사용자가 취소
        }
        window.alert(`백업 가져오기 실패: ${e}`);
        console.error(e);
    }
}

async function initTodos() {
    const form = $('todo-form');
    const input = $('new-task');
    const filterEl = document.getElementById('todo-filter');
    const exportBtn = document.getElementById('export-data');
    const importBtn = document.getElementById('import-data');

    if (exportBtn) {
        exportBtn.addEventListener('click', exportData);
    }
    if (importBtn) {
        importBtn.addEventListener('click', importData);
    }

    if (filterEl) {
        filterEl.addEventListener('click', (e) => {
            const btn = e.target?.closest?.('[data-filter]');
            const next = btn?.getAttribute?.('data-filter');
            if (!next) return;
            if (next === currentFilter) return;
            currentFilter = next;

            const buttons = Array.from(filterEl.querySelectorAll('[data-filter]'));
            buttons.forEach((b) => {
                const active = b.getAttribute('data-filter') === currentFilter;
                b.classList.toggle('is-active', active);
                b.setAttribute('aria-pressed', String(active));
            });

            renderTasks(currentTasks);
        });
    }

    const refresh = async () => {
        const tasks = await invokeOrFallback('get_tasks', {}, async () => loadLocalTasks());
        renderTasks(tasks);
    };

    form.addEventListener('submit', async (e) => {
        e.preventDefault();
        const text = input.value.trim();
        if (!text) return;

        const addLocal = async () => {
            const local = loadLocalTasks();
            // 같은 밀리초에 추가돼도 기존 id와 겹치지 않게
            const id = Math.max(Date.now(), ...local.map((x) => Number(x.id) + 1).filter(Number.isFinite));
            const next = [
                {
                    id,
                    text,
                    completed: false,
                    created_at: Math.floor(Date.now() / 1000),
                    completed_at: null,
                },
                ...local,
            ];
            saveLocalTasks(next);
            return next;
        };

        let updated;
        try {
            updated = await invokeOrFallback('add_task', { text }, addLocal);
        } catch (err) {
            // 입력값은 지우지 않고 남겨 둠
            window.alert(`할 일 추가 실패: ${err}`);
            console.error(err);
            return;
        }

        input.value = '';
        input.focus();
        renderTasks(updated);
    });

    await refresh();
}

function formatMsToTime(ms) {
    const totalSecs = Math.floor(ms / 1000);
    const hours = Math.floor(totalSecs / 3600);
    const minutes = Math.floor((totalSecs % 3600) / 60);
    if (hours > 0) {
        return `${hours}시간 ${minutes}분`;
    }
    return `${minutes}분`;
}

function formatMsToSec(ms) {
    return (ms / 1000).toFixed(1);
}

// 로컬 시간대 기준 'YYYY-MM-DD' (toISOString은 UTC 날짜라 KST 오전 9시 전에는 하루 밀림)
function toLocalYmd(date) {
    const y = date.getFullYear();
    const m = String(date.getMonth() + 1).padStart(2, '0');
    const d = String(date.getDate()).padStart(2, '0');
    return `${y}-${m}-${d}`;
}

// 'YYYY-MM-DD'를 로컬 자정으로 파싱 (new Date('YYYY-MM-DD')는 UTC 자정으로 파싱됨)
function parseLocalYmd(ymd) {
    const [y, m, d] = ymd.split('-').map(Number);
    return new Date(y, m - 1, d);
}

// get_range_stats와 같은 모양의 빈 통계 (Tauri가 없을 때)
function emptyRangeStats(startDate, endDate) {
    const daily_stats = [];
    const end = parseLocalYmd(endDate);
    for (let d = parseLocalYmd(startDate); d <= end; d.setDate(d.getDate() + 1)) {
        daily_stats.push({
            date: toLocalYmd(d),
            tasks_completed: 0,
            tasks_created: 0,
            focus_time_ms: 0,
            lap_count: 0,
            avg_lap_time_ms: null,
        });
    }
    return {
        start_date: startDate,
        end_date: endDate,
        total_tasks_completed: 0,
        total_tasks_created: 0,
        total_focus_time_ms: 0,
        total_lap_count: 0,
        avg_lap_time_ms: null,
        daily_stats,
    };
}

async function setupStats() {
    const toggleBtn = document.getElementById('toggle-stats');
    const panel = document.getElementById('stats-panel');
    const startDateInput = document.getElementById('stats-start-date');
    const endDateInput = document.getElementById('stats-end-date');
    const loadBtn = document.getElementById('stats-load');
    const exportBtn = document.getElementById('stats-export-csv');
    const content = document.getElementById('stats-content');
    const empty = document.getElementById('stats-empty');
    const chartTasksCanvas = document.getElementById('stats-chart-tasks');
    const chartFocusCanvas = document.getElementById('stats-chart-focus');

    let chartTasks = null;
    let chartFocus = null;

    if (!window.Chart) {
        console.error('Chart.js가 로드되지 않았습니다.');
        return;
    }

    // Chart.js 기본 색상 설정
    const chartColors = {
        completed: 'rgba(124, 92, 255, 0.8)',
        created: 'rgba(47, 230, 199, 0.8)',
        focus: 'rgba(92, 124, 255, 0.8)',
    };

    // 기본 범위: 오늘 포함 최근 7일
    const today = new Date();
    const weekAgo = new Date(today);
    weekAgo.setDate(today.getDate() - 6);
    startDateInput.value = toLocalYmd(weekAgo);
    endDateInput.value = toLocalYmd(today);

    // 입력된 기간을 검증하고, 잘못되면 알린 뒤 null 반환
    const readRange = () => {
        const startDate = startDateInput.value;
        const endDate = endDateInput.value;
        if (!startDate || !endDate) {
            window.alert('시작일과 종료일을 모두 선택해주세요.');
            return null;
        }
        // 'YYYY-MM-DD'는 문자열 비교가 날짜 순서와 같음
        if (startDate > endDate) {
            window.alert('시작일이 종료일보다 늦습니다. 기간을 다시 선택해주세요.');
            return null;
        }
        return { startDate, endDate };
    };

    const loadStats = async () => {
        const range = readRange();
        if (!range) return;
        const { startDate, endDate } = range;

        try {
            // 기간 전체를 한 번에 조회 (Tauri가 없으면 0으로 채운 빈 통계)
            const stats = await invokeOrFallback(
                'get_range_stats',
                { start_date: startDate, end_date: endDate },
                async () => emptyRangeStats(startDate, endDate),
            );
            const dailyStats = stats.daily_stats;

            // UI 업데이트
            document.getElementById('stat-completed').textContent = String(stats.total_tasks_completed);
            document.getElementById('stat-created').textContent = String(stats.total_tasks_created);
            document.getElementById('stat-focus').textContent = formatMsToTime(stats.total_focus_time_ms);
            document.getElementById('stat-avg-lap').textContent =
                stats.avg_lap_time_ms == null ? '-' : `${formatMsToSec(stats.avg_lap_time_ms)}초`;

            // 차트 데이터 준비
            const labels = dailyStats.map((s) => {
                const [, m, d] = s.date.split('-').map(Number);
                return `${m}/${d}`;
            });
            const completedData = dailyStats.map((s) => s.tasks_completed || 0);
            const createdData = dailyStats.map((s) => s.tasks_created || 0);
            const focusData = dailyStats.map((s) => Math.floor((s.focus_time_ms || 0) / 60000)); // 분 단위

            // 할 일 차트 (Bar)
            if (chartTasks) {
                chartTasks.destroy();
            }
            chartTasks = new Chart(chartTasksCanvas, {
                type: 'bar',
                data: {
                    labels,
                    datasets: [
                        {
                            label: '완료된 할 일',
                            data: completedData,
                            backgroundColor: chartColors.completed,
                            borderColor: chartColors.completed,
                            borderWidth: 1,
                        },
                        {
                            label: '생성된 할 일',
                            data: createdData,
                            backgroundColor: chartColors.created,
                            borderColor: chartColors.created,
                            borderWidth: 1,
                        },
                    ],
                },
                options: {
                    responsive: true,
                    maintainAspectRatio: false,
                    plugins: {
                        legend: {
                            labels: {
                                color: 'rgba(255, 255, 255, 0.8)',
                                font: { size: 11 },
                            },
                        },
                    },
                    scales: {
                        y: {
                            beginAtZero: true,
                            ticks: {
                                color: 'rgba(255, 255, 255, 0.6)',
                                font: { size: 10 },
                            },
                            grid: {
                                color: 'rgba(255, 255, 255, 0.1)',
                            },
                        },
                        x: {
                            ticks: {
                                color: 'rgba(255, 255, 255, 0.6)',
                                font: { size: 10 },
                            },
                            grid: {
                                color: 'rgba(255, 255, 255, 0.1)',
                            },
                        },
                    },
                },
            });

            // 집중 시간 차트 (Line)
            if (chartFocus) {
                chartFocus.destroy();
            }
            chartFocus = new Chart(chartFocusCanvas, {
                type: 'line',
                data: {
                    labels,
                    datasets: [
                        {
                            label: '집중 시간 (분)',
                            data: focusData,
                            borderColor: chartColors.focus,
                            backgroundColor: 'rgba(92, 124, 255, 0.1)',
                            borderWidth: 2,
                            fill: true,
                            tension: 0.4,
                        },
                    ],
                },
                options: {
                    responsive: true,
                    maintainAspectRatio: false,
                    plugins: {
                        legend: {
                            labels: {
                                color: 'rgba(255, 255, 255, 0.8)',
                                font: { size: 11 },
                            },
                        },
                    },
                    scales: {
                        y: {
                            beginAtZero: true,
                            ticks: {
                                color: 'rgba(255, 255, 255, 0.6)',
                                font: { size: 10 },
                            },
                            grid: {
                                color: 'rgba(255, 255, 255, 0.1)',
                            },
                        },
                        x: {
                            ticks: {
                                color: 'rgba(255, 255, 255, 0.6)',
                                font: { size: 10 },
                            },
                            grid: {
                                color: 'rgba(255, 255, 255, 0.1)',
                            },
                        },
                    },
                },
            });

            content.querySelector('.stats-grid').hidden = false;
            content.querySelector('.stats-charts').hidden = false;
            empty.hidden = true;
        } catch (e) {
            window.alert(`통계 조회 실패: ${e}`);
            console.error(e);
        }
    };

    const exportCsv = async () => {
        const range = readRange();
        if (!range) return;
        const { startDate, endDate } = range;

        if (typeof tauriInvoke !== 'function' || !tauriDialog) {
            window.alert('Tauri 환경에서만 CSV 내보내기가 가능합니다.');
            return;
        }

        try {
            const defaultName = `todo_stats_${startDate}_${endDate}.csv`;
            const filePath = await tauriDialog.save({
                defaultPath: defaultName,
                filters: [{ name: 'CSV', extensions: ['csv'] }],
            });
            if (!filePath || (Array.isArray(filePath) && filePath.length === 0)) {
                return; // 사용자가 취소
            }
            const path = Array.isArray(filePath) ? filePath[0] : filePath;
            const savedPath = await tauriInvoke('export_stats_csv', {
                start_date: startDate,
                end_date: endDate,
                file_path: path,
            });
            window.alert(`CSV가 저장되었습니다:\n${savedPath}`);
        } catch (e) {
            if (String(e).includes('cancelled') || String(e).includes('user cancelled')) {
                return; // 사용자가 취소
            }
            window.alert(`CSV 내보내기 실패: ${e}`);
            console.error(e);
        }
    };

    if (toggleBtn && panel) {
        toggleBtn.addEventListener('click', () => {
            const isOpen = !panel.hidden;
            panel.hidden = isOpen;
            toggleBtn.setAttribute('aria-expanded', String(!isOpen));
            // 열 때마다 현재 기간으로 바로 조회
            if (!isOpen) loadStats();
        });
    }

    if (loadBtn) {
        loadBtn.addEventListener('click', loadStats);
    }

    if (exportBtn) {
        exportBtn.addEventListener('click', exportCsv);
    }

    // 초기 상태
    if (content) {
        content.querySelector('.stats-grid').hidden = true;
        content.querySelector('.stats-charts').hidden = true;
    }
}

// 시작 시 저장 데이터를 불러오지 못했으면 한 번 알림
async function reportLoadFailure() {
    if (typeof tauriInvoke !== 'function') return;
    const failure = await tauriInvoke('take_load_failure');
    if (!failure) return;

    const lines = [
        '저장된 데이터를 불러오지 못해 빈 상태로 시작합니다.',
        `원인: ${failure.reason}`,
    ];
    if (failure.preserved_path) {
        lines.push(`기존 데이터 파일은 삭제하지 않고 다음 위치에 보존했습니다:\n${failure.preserved_path}`);
    }
    if (failure.writes_blocked) {
        lines.push('기존 데이터 파일을 보존하지 못해, 이번 실행에서는 변경 사항이 저장되지 않습니다.');
    }
    window.alert(lines.join('\n\n'));
}

startClock();
setupStopwatch()
    .then((ui) => {
        stopwatchUi = ui;
    })
    .catch((e) => console.error(e));
initTodos().catch((e) => console.error(e));
setupStats().catch((e) => console.error(e));
reportLoadFailure().catch((e) => console.error(e));
