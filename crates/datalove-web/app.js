// Datalove REPL Web Application

import init, { WebReplApp } from './pkg/datalove_web.js';

let app = null;

// Main entry point.
async function main() {
    try {
        // Initialize WASM module.
        await init();

        // Create REPL app.
        app = new WebReplApp();

        // Setup event listeners.
        setupInput();
        setupKeyboard();
        setupModals();

        // Start render loop.
        requestAnimationFrame(update);

        console.log('Datalove REPL initialized');
    } catch (error) {
        console.error('Failed to initialize REPL:', error);
        document.getElementById('app').innerHTML = `
            <div style="color: red; padding: 2rem;">
                <h2>Failed to initialize REPL</h2>
                <pre>${error}</pre>
            </div>
        `;
    }
}

// Setup input event handlers.
function setupInput() {
    const input = document.getElementById('input-area');

    input.addEventListener('keydown', (e) => {
        if (e.key === 'Enter' && !e.altKey && !e.shiftKey && !app.multiline_mode()) {
            e.preventDefault();
            submitInput();
        } else if (e.key === 'Enter' && e.altKey) {
            e.preventDefault();
            submitInput();
        }
    });
}

// Setup global keyboard handlers.
function setupKeyboard() {
    document.addEventListener('keydown', (e) => {
        if (e.key === 'Escape') {
            if (app.crash_modal_is_open()) {
                app.dismiss_crash_modal();
            } else if (app.menu_is_open()) {
                app.close_menu();
            } else {
                app.open_menu();
            }
        }
    });
}

// Setup modal button handlers.
function setupModals() {
    document.getElementById('menu-resume').onclick = () => {
        app.resume_from_menu();
    };

    document.getElementById('menu-exit').onclick = () => {
        app.exit_from_menu();
    };

    document.getElementById('crash-close').onclick = () => {
        app.dismiss_crash_modal();
    };
}

// Submit current input to REPL.
function submitInput() {
    const input = document.getElementById('input-area');
    const text = input.value;

    const action = app.submit_input(text);
    processUiAction(action);
}

// Main update loop.
function update() {
    // Poll for results from worker.
    const actions = app.poll_results();
    for (const action of actions) {
        processUiAction(action);
    }

    // Render current state.
    renderHistory();
    renderEnvironment();
    renderModals();

    // Check exit condition.
    if (app.should_exit()) {
        console.log('REPL exit requested');
        document.getElementById('app').innerHTML = `
            <div style="padding: 2rem; text-align: center;">
                <h2>Datalove REPL</h2>
                <p>Session ended</p>
            </div>
        `;
        return;
    }

    // Continue loop.
    requestAnimationFrame(update);
}

// Process a UiAction from the core app.
function processUiAction(action) {
    switch (action) {
        case 'None':
            break;
        case 'ClearInput':
            clearInput();
            break;
        default:
            if (action.SetMultilineInput) {
                setMultilineInput(action.SetMultilineInput.lines);
            }
            break;
    }
}

// Clear the input area.
function clearInput() {
    const input = document.getElementById('input-area');
    const inputPanel = document.getElementById('input-panel');

    input.value = '';
    inputPanel.classList.remove('multiline');
    document.getElementById('submit-hint').textContent = '[Enter]';
}

// Set multiline input mode.
function setMultilineInput(lines) {
    const input = document.getElementById('input-area');
    const inputPanel = document.getElementById('input-panel');

    input.value = lines.join('\n');
    inputPanel.classList.add('multiline');
    document.getElementById('submit-hint').textContent = '[Alt+Enter]';

    // Move cursor to end.
    input.selectionStart = input.value.length;
    input.selectionEnd = input.value.length;
    input.focus();
}

// Render history entries.
function renderHistory() {
    const history = app.get_history();
    const container = document.getElementById('history-content');

    // Clear container.
    container.innerHTML = '';

    // Render each entry.
    for (const entry of history) {
        const div = createHistoryEntry(entry);
        container.appendChild(div);
    }

    // Auto-scroll to bottom.
    const panel = document.getElementById('history-panel');
    panel.scrollTop = panel.scrollHeight;
}

// Create a history entry element.
function createHistoryEntry(entry) {
    const div = document.createElement('div');
    div.className = 'history-entry';

    // Create prompt element.
    const prompt = document.createElement('div');
    prompt.className = 'history-prompt';
    prompt.textContent = formatPrompt(entry);
    div.appendChild(prompt);

    // Create result element if available.
    if (entry.eval_result) {
        const result = document.createElement('div');
        result.className = 'history-result';
        result.innerHTML = formatEvalResult(entry.eval_result);
        div.appendChild(result);
    } else if (entry.status === 'Parsing') {
        const result = document.createElement('div');
        result.className = 'history-result status-parsing';
        result.textContent = '⏱ parsing...';
        div.appendChild(result);
    } else if (entry.status && entry.status.Evaluating) {
        const result = document.createElement('div');
        result.className = 'history-result status-parsing';
        result.textContent = '⏱ evaluating...';
        div.appendChild(result);
    }

    return div;
}

// Format the prompt text.
function formatPrompt(entry) {
    // Truncate multiline inputs.
    let text = entry.input;
    if (text.includes('\n')) {
        const firstLine = text.split('\n')[0];
        text = firstLine + '...';
    }

    return '> ' + text;
}

// Format evaluation result.
function formatEvalResult(evalResult) {
    if (evalResult.Error) {
        return `<span class="status-error">✗ ${escapeHtml(evalResult.Error)}</span>`;
    } else if (evalResult.SuccessLet) {
        const { name, ty, value } = evalResult.SuccessLet;
        return `<span class="status-success">✓</span> ` +
               `<span class="var-name">${escapeHtml(name)}</span> : ` +
               `<span class="var-type">${escapeHtml(ty)}</span> = ` +
               `${escapeHtml(value)}`;
    } else if (evalResult.SuccessExpr) {
        const { ty, value } = evalResult.SuccessExpr;
        return `<span class="status-success">⇒</span> ` +
               `<span class="var-type">${escapeHtml(ty)}</span> = ` +
               `${escapeHtml(value)}`;
    } else if (evalResult.SuccessFun) {
        const { name } = evalResult.SuccessFun;
        return `<span class="status-success">✓</span> fun ` +
               `<span class="var-name">${escapeHtml(name)}</span>`;
    } else if (evalResult.Nothing) {
        return '<span class="status-success">✓</span>';
    } else if (evalResult.CallerInterpret) {
        return '<span class="status-success">✓</span>';
    } else if (evalResult.CrashReset) {
        return `<span class="status-error">💥 ${escapeHtml(evalResult.CrashReset)}</span>`;
    } else {
        return '<span>unknown result</span>';
    }
}

// Render environment table.
function renderEnvironment() {
    const env = app.get_environment();
    const table = document.getElementById('env-table');
    const emptyMsg = document.getElementById('env-empty');

    if (env.length === 0) {
        table.style.display = 'none';
        emptyMsg.style.display = 'block';
        return;
    }

    table.style.display = 'table';
    emptyMsg.style.display = 'none';

    // Clear table.
    table.innerHTML = '';

    // Render each variable.
    for (const [name, ty, value] of env) {
        const tr = table.insertRow();
        tr.innerHTML = `
            <td class="var-name">${escapeHtml(name)}</td>
            <td class="var-type">${escapeHtml(ty)}</td>
            <td class="var-value">${escapeHtml(value)}</td>
        `;
    }
}

// Render modals.
function renderModals() {
    // Menu modal.
    const menuModal = document.getElementById('menu-modal');
    const menuOpen = app.menu_is_open();
    menuModal.classList.toggle('hidden', !menuOpen);

    // Crash modal.
    const crashModal = document.getElementById('crash-modal');
    const crashOpen = app.crash_modal_is_open();
    crashModal.classList.toggle('hidden', !crashOpen);

    if (crashOpen) {
        const message = app.crash_modal_message();
        if (message) {
            document.getElementById('crash-message').textContent = message;
        }
    }
}

// Escape HTML special characters.
function escapeHtml(text) {
    const div = document.createElement('div');
    div.textContent = text;
    return div.innerHTML;
}

// Start the app.
main();
