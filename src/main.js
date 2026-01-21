const tauriInvoke = globalThis?.__TAURI__?.tauri?.invoke;

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
        const lap_totals_ms = Array.isArray(parsed.lap_totals_ms) ? parsed.lap_totals_ms.map(Number) : [];
        return { elapsed_ms: Math.max(0, elapsed_ms), lap_totals_ms: lap_totals_ms.filter((n) => Number.isFinite(n) && n >= 0) };
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
        await invokeOrFallback(
            'set_stopwatch_state',
            { stopwatch: snapshot },
            async () => {
                saveLocalStopwatch(snapshot);
                return snapshot;
            },
        );
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
        invokeOrFallback(
            'clear_stopwatch_state',
            {},
            async () => {
                localStorage.removeItem(STOPWATCH_STORAGE_KEY);
                return true;
            },
        ).catch((e) => console.error(e));
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
    const loaded = await invokeOrFallback(
        'get_stopwatch_state',
        {},
        async () => loadLocalStopwatch(),
    );
    if (loaded && typeof loaded === 'object') {
        elapsed = Number(loaded.elapsed_ms ?? 0) || 0;
        lapTotals = Array.isArray(loaded.lap_totals_ms) ? loaded.lap_totals_ms.map(Number).filter((n) => Number.isFinite(n) && n >= 0) : [];
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
                const local = loadLocalTasks().map((x) =>
                    x.id === t.id ? { ...x, completed: !x.completed } : x,
                );
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

async function initTodos() {
    const form = $('todo-form');
    const input = $('new-task');
    const filterEl = document.getElementById('todo-filter');

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
                { id, text, completed: false, created_at: Math.floor(Date.now() / 1000) },
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

startClock();
setupStopwatch().catch((e) => console.error(e));
initTodos().catch((e) => console.error(e));
