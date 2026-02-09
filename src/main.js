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

function loadLocalStopwatch() {
    try {
        const raw = localStorage.getItem(STOPWATCH_STORAGE_KEY);
        const parsed = raw ? JSON.parse(raw) : null;
        if (!parsed) return null;
        const elapsed_ms = Number(parsed.elapsed_ms ?? 0);
        const lap_totals_ms = Array.isArray(parsed.lap_totals_ms)
            ? parsed.lap_totals_ms.map(Number)
            : [];
        return {
            elapsed_ms: Math.max(0, elapsed_ms),
            lap_totals_ms: lap_totals_ms.filter((n) => Number.isFinite(n) && n >= 0),
        };
    } catch {
        return null;
    }
}

function saveLocalStopwatch(state) {
    try {
        localStorage.setItem(STOPWATCH_STORAGE_KEY, JSON.stringify(state));
    } catch {
        // ignore
    }
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

    let running = false;
    let startAt = 0;
    let elapsed = 0;
    let timer = null;
    let lapTotals = [];

    const getCurrentMs = () => elapsed + (running ? Date.now() - startAt : 0);

    const render = () => {
        timeEl.textContent = formatStopwatch(getCurrentMs());
    };

    const setStatus = (label) => {
        statusEl.textContent = label;
    };

    const renderLaps = () => {
        lapsList.innerHTML = '';
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

    const persistStopwatch = async () => {
        const snapshot = { elapsed_ms: getCurrentMs(), lap_totals_ms: lapTotals.slice() };
        await invokeOrFallback('set_stopwatch_state', { stopwatch: snapshot }, async () => {
            saveLocalStopwatch(snapshot);
            return snapshot;
        });
    };

    const stop = () => {
        if (!running) return;
        running = false;
        elapsed += Date.now() - startAt;
        startAt = 0;
        if (timer) clearInterval(timer);
        timer = null;
        startBtn.textContent = '시작';
        setStatus('paused');
        lapBtn.disabled = getCurrentMs() <= 0;
        render();
        persistStopwatch().catch((e) => console.error(e));
    };

    const start = () => {
        if (running) return;
        running = true;
        startAt = Date.now();
        timer = setInterval(render, 50);
        startBtn.textContent = '일시정지';
        setStatus('running');
        lapBtn.disabled = false;
        render();
    };

    const reset = () => {
        running = false;
        if (timer) clearInterval(timer);
        timer = null;
        startAt = 0;
        elapsed = 0;
        lapTotals = [];
        startBtn.textContent = '시작';
        setStatus('ready');
        lapBtn.disabled = true;
        render();
        renderLaps();
        invokeOrFallback('clear_stopwatch_state', {}, async () => {
            localStorage.removeItem(STOPWATCH_STORAGE_KEY);
            return true;
        }).catch((e) => console.error(e));
    };

    const clearLaps = () => {
        lapTotals = [];
        renderLaps();
        persistStopwatch().catch((e) => console.error(e));
    };

    const deleteLap = (index) => {
        const i = Number(index);
        if (!Number.isFinite(i)) return;
        if (i < 0 || i >= lapTotals.length) return;
        lapTotals = lapTotals.filter((_, idx) => idx !== i);
        renderLaps();
        persistStopwatch().catch((e) => console.error(e));
    };

    const lap = () => {
        const current = getCurrentMs();
        if (current <= 0) return;
        lapTotals.push(current);
        renderLaps();
        persistStopwatch().catch((e) => console.error(e));
    };

    toggleBtn.addEventListener('click', () => {
        const isOpen = !panel.hidden;
        panel.hidden = isOpen;
        toggleBtn.setAttribute('aria-expanded', String(!isOpen));
        if (isOpen) stop();
    });

    startBtn.addEventListener('click', () => {
        if (running) stop();
        else start();
    });

    lapBtn.addEventListener('click', lap);
    resetBtn.addEventListener('click', reset);
    lapsClearBtn.addEventListener('click', clearLaps);

    lapsList.addEventListener('click', (e) => {
        const btn = e.target?.closest?.('[data-lap-index]');
        const idx = btn?.getAttribute?.('data-lap-index');
        if (idx == null) return;
        deleteLap(idx);
    });

    render();
    renderLaps();
    lapBtn.disabled = true;

    // 초기 로드(tauri 우선)
    const loaded = await invokeOrFallback('get_stopwatch_state', {}, async () =>
        loadLocalStopwatch(),
    );
    if (loaded && typeof loaded === 'object') {
        elapsed = Number(loaded.elapsed_ms ?? 0) || 0;
        lapTotals = Array.isArray(loaded.lap_totals_ms)
            ? loaded.lap_totals_ms.map(Number).filter((n) => Number.isFinite(n) && n >= 0)
            : [];
        render();
        renderLaps();
        lapBtn.disabled = elapsed <= 0;
        if (typeof tauriInvoke !== 'function') {
            saveLocalStopwatch({ elapsed_ms: elapsed, lap_totals_ms: lapTotals.slice() });
        }
    }
}

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

async function exportData() {
    if (typeof tauriInvoke !== 'function' || !tauriDialog) {
        window.alert('Tauri 환경에서만 백업 내보내기가 가능합니다.');
        return;
    }
    try {
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
        console.log('Export path:', path);
        console.log('Invoking with:', { file_path: path });
        const savedPath = await tauriInvoke('export_data', { file_path: path });
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
        const imported = await tauriInvoke('import_data', { file_path: path });
        if (imported && imported.tasks) {
            renderTasks(imported.tasks);
            if (imported.stopwatch) {
                // 스탑워치 상태도 복원 (선택적)
                const stopwatchState = imported.stopwatch;
                await invokeOrFallback(
                    'set_stopwatch_state',
                    { stopwatch: stopwatchState },
                    async () => {
                        saveLocalStopwatch({
                            elapsed_ms: stopwatchState.elapsed_ms,
                            lap_totals_ms: stopwatchState.lap_totals_ms,
                        });
                        return stopwatchState;
                    },
                );
                // 스탑워치 UI 새로고침을 위해 페이지 리로드 또는 상태 동기화
                location.reload();
            } else {
                renderTasks(imported.tasks);
            }
            window.alert('백업이 성공적으로 가져와졌습니다.');
        }
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

        const updated = await invokeOrFallback('add_task', { text }, async () => {
            const local = loadLocalTasks();
            const id = Date.now();
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
        });

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

    // 오늘 날짜로 기본 설정
    const today = new Date();
    const weekAgo = new Date(today);
    weekAgo.setDate(today.getDate() - 7);
    startDateInput.value = weekAgo.toISOString().split('T')[0];
    endDateInput.value = today.toISOString().split('T')[0];

    const loadStats = async () => {
        const startDate = startDateInput.value;
        const endDate = endDateInput.value;
        if (!startDate || !endDate) {
            window.alert('시작일과 종료일을 모두 선택해주세요.');
            return;
        }

        try {
            // 일별 통계를 수집
            const dates = [];
            const start = new Date(startDate);
            const end = new Date(endDate);
            for (let d = new Date(start); d <= end; d.setDate(d.getDate() + 1)) {
                dates.push(d.toISOString().split('T')[0]);
            }

            let totalCompleted = 0;
            let totalCreated = 0;
            let totalFocusMs = 0;
            let totalLaps = 0;
            let lapTimes = [];

            const dailyStats = [];
            for (const date of dates) {
                const stats = await invokeOrFallback(
                    'get_daily_stats',
                    { date },
                    async () => {
                        return {
                            tasks_completed: 0,
                            tasks_created: 0,
                            focus_time_ms: 0,
                            lap_count: 0,
                            avg_lap_time_ms: null,
                        };
                    },
                );
                dailyStats.push({ date, ...stats });
                totalCompleted += stats.tasks_completed || 0;
                totalCreated += stats.tasks_created || 0;
                totalFocusMs += stats.focus_time_ms || 0;
                totalLaps += stats.lap_count || 0;
                if (stats.avg_lap_time_ms) {
                    lapTimes.push(stats.avg_lap_time_ms);
                }
            }

            const avgLapMs =
                lapTimes.length > 0
                    ? lapTimes.reduce((a, b) => a + b, 0) / lapTimes.length
                    : null;

            // UI 업데이트
            document.getElementById('stat-completed').textContent = String(totalCompleted);
            document.getElementById('stat-created').textContent = String(totalCreated);
            document.getElementById('stat-focus').textContent = formatMsToTime(totalFocusMs);
            document.getElementById('stat-avg-lap').textContent = avgLapMs
                ? `${formatMsToSec(avgLapMs)}초`
                : '-';

            // 차트 데이터 준비
            const labels = dailyStats.map((s) => {
                const d = new Date(s.date);
                return `${d.getMonth() + 1}/${d.getDate()}`;
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
        const startDate = startDateInput.value;
        const endDate = endDateInput.value;
        if (!startDate || !endDate) {
            window.alert('시작일과 종료일을 모두 선택해주세요.');
            return;
        }

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

startClock();
setupStopwatch().catch((e) => console.error(e));
initTodos().catch((e) => console.error(e));
setupStats().catch((e) => console.error(e));
