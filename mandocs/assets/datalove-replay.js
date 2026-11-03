// A player for recorded Datalove REPL sessions.
//
// A trace is recorded by `datalove repl --load <world> --record <path>`,
// and is the state the REPL shows after each input: what the input came to,
// the session's units with their statuses, the environment, and each unit's
// detail. The player steps through it and shows it the way the REPL does,
// with no engine behind it.
//
// A page loads this file, then a `.replay.js` file that registers a trace by
// name, and puts the player wherever an element names that trace:
//
//     <div class="datalove-replay" data-replay="reactive-transactions"></div>
//     <script src="../assets/datalove-replay.js"></script>
//     <script src="../assets/reactive-transactions.replay.js"></script>
//
// `DataloveReplay.mount(element, trace)` puts a player in an element directly.

(function () {
    'use strict';

    const TRACE_VERSION = 1;

    const traces = {};

    // How each status is shown: its class, which colours it, and in words.
    const STATUS = {
        Ran: { cls: 'ran', words: 'ran' },
        NotCompiled: { cls: 'not-compiled', words: 'did not compile' },
        ReturnedEarly: { cls: 'returned-early', words: 'returned early' },
        Blocked: { cls: 'blocked', words: 'blocked' },
    };

    const STYLE = `
.dlr {
    --dlr-fg: var(--fg, #222);
    --dlr-bg: var(--code-bg, #f4f1ec);
    --dlr-dim: #8a8580;
    --dlr-border: color-mix(in srgb, var(--dlr-fg), transparent 80%);
    --dlr-green: #3c9a46;
    --dlr-red: #d0403a;
    --dlr-yellow: #b8901a;
    --dlr-magenta: #b8459e;
    --dlr-cyan: #23879a;
    --dlr-type: #9a7612;
    font-family: var(--font-code, monospace);
    font-size: 0.9rem;
    line-height: 1.35;
    color: var(--dlr-fg);
    border: 1px solid var(--dlr-border);
    border-radius: 4px;
    margin: 1.5rem 0;
    background: var(--dlr-bg);
}
@media (prefers-color-scheme: dark) {
    .dlr {
        --dlr-fg: var(--fg, #ddd);
        --dlr-bg: var(--code-bg, #1e1d1b);
        --dlr-green: #62b65a;
        --dlr-red: #e45a52;
        --dlr-yellow: #d2b13c;
        --dlr-magenta: #d16ac0;
        --dlr-cyan: #3fb6c4;
        --dlr-type: #d2b13c;
    }
}
.dlr:focus { outline: none; }
.dlr:focus-visible { outline: 2px solid var(--dlr-cyan); }
.dlr pre {
    margin: 0;
    padding: 0;
    background: none;
    border: none;
    font: inherit;
    line-height: inherit;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
}
.dlr-title {
    color: var(--dlr-dim);
    font-size: 0.8em;
    padding: 0.3em 0.6em;
    border-bottom: 1px solid var(--dlr-border);
}
.dlr-code {
    height: 22em;
    overflow-y: auto;
    padding: 0.2em 0;
}
.dlr-empty { color: var(--dlr-dim); padding: 0.4em 0.6em; }
.dlr-unit {
    display: flex;
    align-items: stretch;
    cursor: pointer;
    border-bottom: 1px solid var(--dlr-border);
    padding: 0.15em 0;
}
.dlr-unit:last-child { border-bottom: none; }
.dlr-unit.dlr-selected { background: color-mix(in srgb, var(--dlr-cyan), transparent 88%); }
.dlr-gutter {
    width: 2.2em;
    text-align: right;
    padding-right: 0.4em;
    color: var(--dlr-dim);
    flex: none;
}
.dlr-selected .dlr-gutter { color: var(--dlr-fg); font-weight: bold; }
.dlr-bar {
    position: relative;
    width: 0.8em;
    flex: none;
    margin-right: 0.6em;
    min-height: 1.35em;
}
.dlr-bar::before {
    content: '';
    position: absolute;
    left: 0; top: 0; bottom: 0;
    width: 50%;
    background: var(--dlr-status);
}
.dlr-mark {
    position: absolute;
    right: 0;
    width: 50%;
    height: 0.68em;
    background: var(--dlr-status);
}
.dlr-mark-compiled { top: 0; }
.dlr-mark-ran { bottom: 0; }
.dlr-unit pre { flex: 1; min-width: 0; padding-right: 0.6em; }
.dlr-status-ran { --dlr-status: var(--dlr-green); }
.dlr-status-not-compiled { --dlr-status: var(--dlr-red); }
.dlr-status-returned-early { --dlr-status: var(--dlr-yellow); }
.dlr-status-blocked { --dlr-status: var(--dlr-magenta); }
.dlr-input {
    border-top: 1px solid var(--dlr-border);
    border-bottom: 1px solid var(--dlr-border);
}
.dlr-input-head {
    display: flex;
    align-items: center;
    gap: 0.6em;
    padding: 0.3em 0.6em;
    border-bottom: 1px solid var(--dlr-border);
    color: var(--dlr-dim);
    font-size: 0.8em;
}
.dlr-input-head .dlr-count { margin-left: auto; }
.dlr button {
    font: inherit;
    color: var(--dlr-fg);
    background: none;
    border: 1px solid var(--dlr-border);
    border-radius: 3px;
    padding: 0.15em 0.7em;
    cursor: pointer;
}
.dlr button:disabled { color: var(--dlr-dim); cursor: default; }
.dlr button:not(:disabled):hover { border-color: var(--dlr-cyan); }
.dlr-input pre {
    padding: 0.4em 0.6em;
    max-height: 12em;
    overflow-y: auto;
}
.dlr-prompt { color: var(--dlr-cyan); }
.dlr-tabs {
    display: flex;
    border-bottom: 1px solid var(--dlr-border);
}
.dlr .dlr-tab {
    border: none;
    border-radius: 0;
    border-right: 1px solid var(--dlr-border);
    padding: 0.3em 0.9em;
    color: var(--dlr-dim);
}
.dlr .dlr-tab.dlr-shown { color: var(--dlr-yellow); font-weight: bold; }
.dlr-pane {
    height: 16em;
    overflow-y: auto;
    padding: 0.4em 0.6em;
}
.dlr-env { border-collapse: collapse; width: 100%; font: inherit; margin: 0; }
.dlr-env th, .dlr-env td {
    text-align: left;
    vertical-align: top;
    padding: 0.1em 1em 0.1em 0;
    border: none;
    background: none;
    overflow-wrap: break-word;
}
.dlr-env td:first-child { white-space: nowrap; }
.dlr-env.dlr-names { width: auto; }
.dlr-env th { color: var(--dlr-yellow); font-weight: normal; }
.dlr-name { color: var(--dlr-cyan); }
.dlr-type { color: var(--dlr-type); }
.dlr-dim { color: var(--dlr-dim); }
.dlr-green { color: var(--dlr-green); }
.dlr-red { color: var(--dlr-red); }
.dlr-yellow { color: var(--dlr-yellow); }
.dlr-magenta { color: var(--dlr-magenta); }
.dlr-entry { margin-bottom: 0.7em; }
.dlr-entry > div { padding-left: 4ch; text-indent: -4ch; }
.dlr-entry.dlr-future { opacity: 0.4; }
.dlr-heading { color: var(--dlr-cyan); font-weight: bold; margin-top: 0.8em; }
.dlr-detail-head { display: flex; align-items: stretch; }
.dlr-diagnostics {
    background: #1c1b19;
    color: #d0ccc4;
    padding: 0.5em 0.7em;
    border-radius: 3px;
    margin-top: 0.3em;
    overflow-x: auto;
}
.dlr-diagnostics pre { white-space: pre; overflow-wrap: normal; }
.dlr-error { color: var(--dlr-red); padding: 0.6em; }
`;

    function addStyle() {
        if (document.getElementById('dlr-style')) {
            return;
        }
        const style = document.createElement('style');
        style.id = 'dlr-style';
        style.textContent = STYLE;
        document.head.appendChild(style);
    }

    // Make an element: `h('div', { class: 'x' }, child, ...)`. A child is a
    // node, a string, or an array of either; `null` is skipped.
    function h(tag, attrs, ...children) {
        const el = document.createElement(tag);
        for (const [key, value] of Object.entries(attrs || {})) {
            if (key.startsWith('on')) {
                el.addEventListener(key.slice(2), value);
            } else if (value !== null && value !== undefined && value !== false) {
                el.setAttribute(key, value === true ? '' : value);
            }
        }
        append(el, children);
        return el;
    }

    function append(el, children) {
        for (const child of children) {
            if (child === null || child === undefined) {
                continue;
            } else if (Array.isArray(child)) {
                append(el, child);
            } else if (typeof child === 'string') {
                el.appendChild(document.createTextNode(child));
            } else {
                el.appendChild(child);
            }
        }
    }

    function span(cls, text) {
        return h('span', { class: cls }, text);
    }

    // The one variant of a serialized Rust enum: `{"Ran": [...]}` is
    // `['Ran', [...]]`, and a variant without fields, `"Reset"`, is
    // `['Reset', null]`.
    function variant(value) {
        if (typeof value === 'string') {
            return [value, null];
        }
        const [key] = Object.keys(value);
        return [key, value[key]];
    }

    // Datalove source, highlighted when the page has Prism and the language.
    function code(source) {
        const pre = h('pre', {});
        const prism = window.Prism;
        if (prism && prism.languages && prism.languages.datalove) {
            const el = h('code', { class: 'language-datalove' });
            el.innerHTML = prism.highlight(source, prism.languages.datalove, 'datalove');
            pre.appendChild(el);
        } else {
            pre.appendChild(h('code', {}, source));
        }
        return pre;
    }

    function blockedReason(on) {
        if (on.length === 1) {
            return `not run: it reads from unit ${on[0]}, which did not finish`;
        }
        const rest = on.slice(0, -1).join(', ');
        return `not run: it reads from units ${rest} and ${on[on.length - 1]}, which did not finish`;
    }

    // What a unit came to, in a line's worth: its mark, its colour class, and
    // what it says.
    function outcome(evaluation) {
        const [kind, value] = variant(evaluation);
        switch (kind) {
        case 'Ran': {
            if (value.length === 0) {
                return ['✓', 'dlr-green', 'ran'];
            }
            const bound = value.map((binding) => {
                const [what, fields] = variant(binding);
                if (what === 'Function') {
                    return `fun ${fields.name}`;
                }
                return fields.value === null ? `${fields.name}, given away` : `${fields.name} = ${fields.value}`;
            });
            return ['✓', 'dlr-green', bound.join(', ')];
        }
        case 'Value':
            return ['⇒', 'dlr-green', `${value.ty} = ${value.value}`];
        case 'NotCompiled':
            return ['✗', 'dlr-red', `error: ${value}`];
        case 'ReturnedEarly':
            return ['✗', 'dlr-yellow', `returned early: ${value}`];
        case 'Blocked':
            return ['✗', 'dlr-magenta', blockedReason(value.on)];
        case 'RuntimeError':
            return ['✗', 'dlr-red', `runtime error, and the session was reset: ${value}`];
        case 'Crashed':
            return ['✗', 'dlr-red', `the engine panicked, and the session was reset: ${value}`];
        default:
            throw new Error(`unknown evaluation ${kind}`);
        }
    }

    function marked(mark, cls, text) {
        return h('div', { class: cls }, `${mark} `, text);
    }

    // One binding a fragment defined, with its type.
    function bindingLine(binding) {
        const [what, fields] = variant(binding);
        const check = span('dlr-green', '✓ ');
        if (what === 'Function') {
            return h('div', {}, check, 'fun ', span('dlr-name', fields.name));
        }
        return h('div', {}, check, span('dlr-name', fields.name), ': ', span('dlr-type', fields.ty),
            fields.value === null ? span('dlr-dim', ' -- given away') : ` = ${fields.value}`);
    }

    // The lines saying what an input came to, as the History tab has them.
    function outcomeLines(result) {
        const [kind, value] = variant(result);
        switch (kind) {
        case 'Unit': {
            const [evalKind, bindings] = variant(value);
            if (evalKind === 'Ran') {
                return bindings.length === 0 ? [marked('✓', 'dlr-green', 'ran')] : bindings.map(bindingLine);
            }
            const [mark, cls, said] = outcome(value);
            return [marked(mark, cls, said)];
        }
        case 'Edited': {
            const lines = value.map((step) => {
                if (step.eval === null) {
                    return marked('·', 'dlr-dim', `unit ${step.unit}: compiled again`);
                }
                const [mark, cls, said] = outcome(step.eval);
                return h('div', { class: cls }, `${mark} `, span('dlr-dim', `unit ${step.unit}: `), said);
            });
            if (value.every((step) => step.eval === null)) {
                lines.unshift(marked('✓', 'dlr-green', 'no other unit reran'));
            }
            return lines;
        }
        case 'Reset':
            return [marked('✓', 'dlr-green', 'the session was reset')];
        case 'CommandError':
            return [marked('✗', 'dlr-red', `error: ${value}`)];
        case 'Help':
            return [marked('ℹ', 'dlr-cyan', 'shown in the Help tab')];
        case 'Saved':
            return [marked('✓', 'dlr-green', `saved ${value.inputs} inputs to ${value.path}`)];
        case 'Loaded':
            return [marked('✓', 'dlr-green', `loaded ${value.inputs} inputs`)];
        default:
            throw new Error(`unknown outcome ${kind}`);
        }
    }

    // A unit's bar: the left half its status, the top right corner whether
    // the last step compiled it, and the bottom right whether it ran it.
    function bar(unit) {
        return h('div', { class: `dlr-bar dlr-status-${STATUS[unit.status].cls}` },
            unit.compiled ? h('span', { class: 'dlr-mark dlr-mark-compiled' }) : null,
            unit.ran ? h('span', { class: 'dlr-mark dlr-mark-ran' }) : null);
    }

    // The colours of xterm's 256, which ariadne's diagnostics use.
    function xterm(n) {
        const base = ['#000000', '#cd3131', '#0dbc79', '#e5e510', '#2472c8', '#bc3fbc', '#11a8cd', '#e5e5e5',
            '#666666', '#f14c4c', '#23d18b', '#f5f543', '#3b8eea', '#d670d6', '#29b8db', '#ffffff'];
        if (n < 16) {
            return base[n];
        }
        if (n < 232) {
            const levels = [0, 95, 135, 175, 215, 255];
            const i = n - 16;
            const [r, g, b] = [Math.floor(i / 36), Math.floor(i / 6) % 6, i % 6].map((c) => levels[c]);
            return `rgb(${r}, ${g}, ${b})`;
        }
        const gray = 8 + 10 * (n - 232);
        return `rgb(${gray}, ${gray}, ${gray})`;
    }

    // Text with ANSI colour escapes, as spans. Escapes other than colour and
    // weight are dropped.
    function ansi(text) {
        const nodes = [];
        const re = /\x1b\[([0-9;?]*)([A-Za-z])/g;
        let color = null;
        let bold = false;
        let last = 0;
        const flush = (end) => {
            if (end > last) {
                const el = span(null, text.slice(last, end));
                if (color) {
                    el.style.color = color;
                }
                if (bold) {
                    el.style.fontWeight = 'bold';
                }
                nodes.push(el);
            }
        };
        let match;
        while ((match = re.exec(text)) !== null) {
            flush(match.index);
            last = re.lastIndex;
            if (match[2] !== 'm') {
                continue;
            }
            const codes = match[1] === '' ? [0] : match[1].split(';').map(Number);
            for (let i = 0; i < codes.length; i++) {
                const c = codes[i];
                if (c === 0) {
                    color = null;
                    bold = false;
                } else if (c === 1) {
                    bold = true;
                } else if (c === 22) {
                    bold = false;
                } else if (c === 39) {
                    color = null;
                } else if (c >= 30 && c <= 37) {
                    color = xterm(c - 30);
                } else if (c >= 90 && c <= 97) {
                    color = xterm(c - 90 + 8);
                } else if (c === 38 && codes[i + 1] === 5) {
                    color = xterm(codes[i + 2]);
                    i += 2;
                } else if (c === 38 && codes[i + 1] === 2) {
                    color = `rgb(${codes[i + 2]}, ${codes[i + 3]}, ${codes[i + 4]})`;
                    i += 4;
                }
            }
        }
        flush(text.length);
        return nodes;
    }

    function resets(result) {
        const [kind, value] = variant(result);
        const evalResets = (evaluation) => ['RuntimeError', 'Crashed'].includes(variant(evaluation)[0]);
        return kind === 'Reset'
            || (kind === 'Unit' && evalResets(value))
            || (kind === 'Edited' && value.some((step) => step.eval !== null && evalResets(step.eval)));
    }

    function isUnitInput(result) {
        return variant(result)[0] === 'Unit';
    }

    class Player {
        constructor(root, trace) {
            if (trace.version !== TRACE_VERSION) {
                throw new Error(`a trace of version ${trace.version}, and the player plays ${TRACE_VERSION}`);
            }
            addStyle();
            this.root = root;
            this.trace = trace;
            // The steps shown, from none to all of them.
            this.shown = 0;
            this.selected = null;
            this.tab = 'Environment';

            root.classList.add('dlr');
            root.tabIndex = 0;
            root.replaceChildren();
            root.addEventListener('keydown', (event) => this.key(event));

            this.code = h('div', { class: 'dlr-code' });
            this.back = h('button', { type: 'button', onclick: () => this.go(this.shown - 1) }, '‹ Back');
            this.forward = h('button', { type: 'button', onclick: () => this.go(this.shown + 1) }, 'Run ›');
            this.count = span('dlr-count', '');
            this.input = h('pre', {});
            this.tabs = h('div', { class: 'dlr-tabs' });
            this.pane = h('div', { class: 'dlr-pane' });
            root.append(
                h('div', { class: 'dlr-title' }, 'Code'),
                this.code,
                h('div', { class: 'dlr-input' },
                    h('div', { class: 'dlr-input-head' }, this.back, this.forward, span(null, 'Next input'), this.count),
                    this.input),
                this.tabs,
                this.pane,
            );
            this.go(0);
        }

        key(event) {
            const moves = { ArrowLeft: this.shown - 1, ArrowRight: this.shown + 1, Home: 0, End: this.trace.steps.length };
            if (event.key in moves) {
                event.preventDefault();
                this.go(moves[event.key]);
            }
        }

        // The step whose session is shown, or `null` before the first.
        step() {
            return this.shown === 0 ? null : this.trace.steps[this.shown - 1];
        }

        units() {
            const step = this.step();
            return step === null ? [] : step.units;
        }

        // Show the session after the first `shown` steps, selecting the unit
        // and the tab the REPL would.
        go(shown) {
            if (shown < 0 || shown > this.trace.steps.length) {
                return;
            }
            this.shown = shown;
            const step = this.step();
            const units = this.units();
            const has = (number) => units.some((unit) => unit.number === number);
            if (step === null || units.length === 0) {
                this.selected = null;
            } else {
                const [kind, value] = variant(step.outcome);
                if (kind === 'Unit') {
                    this.selected = units[units.length - 1].number;
                } else if (kind === 'Edited' && value.length > 0 && has(value[0].unit)) {
                    this.selected = value[0].unit;
                } else if (!has(this.selected)) {
                    this.selected = units[units.length - 1].number;
                }
            }
            this.tab = step !== null && !isUnitInput(step.outcome) ? 'History' : 'Environment';
            const selected = this.selectedUnit();
            if (selected && selected.status !== 'Ran' && (selected.compiled || selected.ran)) {
                this.tab = 'Detail';
            }
            this.render();
        }

        selectedUnit() {
            return this.units().find((unit) => unit.number === this.selected) || null;
        }

        render() {
            this.renderCode();
            this.renderInput();
            this.renderTabs();
            this.renderPane();
        }

        renderCode() {
            const units = this.units();
            if (units.length === 0) {
                this.code.replaceChildren(h('div', { class: 'dlr-empty' }, '(no units)'));
                return;
            }
            let selectedEl = null;
            this.code.replaceChildren(...units.map((unit) => {
                const selected = unit.number === this.selected;
                const el = h('div', {
                    class: `dlr-unit dlr-status-${STATUS[unit.status].cls}${selected ? ' dlr-selected' : ''}`,
                    onclick: () => {
                        this.selected = unit.number;
                        this.tab = 'Detail';
                        this.render();
                    },
                }, h('div', { class: 'dlr-gutter' }, String(unit.number)), bar(unit), code(unit.source));
                if (selected) {
                    selectedEl = el;
                }
                return el;
            }));
            // Scroll the code itself, not the page, to show the selection.
            if (selectedEl) {
                const top = selectedEl.offsetTop - this.code.offsetTop;
                const bottom = top + selectedEl.offsetHeight;
                if (top < this.code.scrollTop) {
                    this.code.scrollTop = top;
                } else if (bottom > this.code.scrollTop + this.code.clientHeight) {
                    this.code.scrollTop = Math.min(top, bottom - this.code.clientHeight);
                }
            }
        }

        renderInput() {
            const steps = this.trace.steps;
            this.back.disabled = this.shown === 0;
            this.forward.disabled = this.shown === steps.length;
            this.count.textContent = `step ${this.shown} of ${steps.length}`;
            if (this.shown === steps.length) {
                this.input.replaceChildren(span('dlr-dim', '(the end of the session)'));
            } else {
                this.input.replaceChildren(span('dlr-prompt', '> '), steps[this.shown].input);
            }
        }

        renderTabs() {
            const tabs = ['Environment', 'History'];
            if (this.units().length > 0) {
                tabs.push('Detail');
            }
            if (!tabs.includes(this.tab)) {
                this.tab = 'Environment';
            }
            this.tabs.replaceChildren(...tabs.map((tab) => h('button', {
                type: 'button',
                class: `dlr-tab${tab === this.tab ? ' dlr-shown' : ''}`,
                onclick: () => {
                    this.tab = tab;
                    this.renderTabs();
                    this.renderPane();
                },
            }, tab)));
        }

        renderPane() {
            this.pane.replaceChildren(...{
                Environment: () => this.environment(),
                History: () => this.history(),
                Detail: () => this.detail(),
            }[this.tab]());
            this.pane.scrollTop = this.tab === 'History' ? this.pane.scrollHeight : 0;
        }

        environment() {
            const step = this.step();
            const bindings = step === null ? [] : step.environment;
            if (bindings.length === 0) {
                return [span('dlr-dim', '(no bindings)')];
            }
            return [h('table', { class: 'dlr-env' },
                h('tr', {}, h('th', {}, 'Name'), h('th', {}, 'Type'), h('th', {}, 'Value')),
                bindings.map((binding) => h('tr', {},
                    h('td', { class: 'dlr-name' }, binding.name),
                    h('td', { class: 'dlr-type' }, binding.ty),
                    binding.value === null ? h('td', { class: 'dlr-dim' }, 'given away') : h('td', {}, binding.value))))];
        }

        // Every input so far and what it came to, as the History tab has
        // them. The REPL forgets what came before a reset; this marks it
        // instead, so a replay can be read from its start.
        history() {
            const steps = this.trace.steps.slice(0, this.shown);
            if (steps.length === 0) {
                return [span('dlr-dim', '(nothing entered)')];
            }
            const entries = [];
            for (const step of steps) {
                const [first, ...rest] = step.input.split('\n');
                entries.push(h('div', { class: 'dlr-entry' },
                    h('div', {}, span('dlr-prompt', '> '), rest.length > 0 ? `${first}…` : first),
                    outcomeLines(step.outcome)));
                if (resets(step.outcome)) {
                    entries.push(h('div', { class: 'dlr-entry dlr-dim' }, '── the session starts again ──'));
                }
            }
            return entries;
        }

        detail() {
            const unit = this.selectedUnit();
            const status = STATUS[unit.status];
            const lines = [h('div', { class: 'dlr-detail-head' }, bar(unit),
                h('span', {}, h('b', {}, `unit ${unit.number}`), span('dlr-dim', ' · '),
                    h('span', { style: 'color: var(--dlr-status)', class: `dlr-status-${status.cls}` }, status.words)))];
            if (unit.compiled || unit.ran) {
                const did = unit.compiled && unit.ran ? 'the last step compiled and ran it'
                    : unit.compiled ? 'the last step compiled it, and did not run it'
                        : 'the last step ran it, and did not compile it';
                lines.push(h('div', { class: 'dlr-dim' }, did));
            }

            const detail = unit.detail;
            const [kind, ran] = variant(unit.outcome);
            if (kind === 'NotCompiled' && detail.diagnostics !== '') {
                lines.push(h('div', { class: 'dlr-heading' }, 'Diagnostics'));
                lines.push(h('div', { class: 'dlr-diagnostics' }, h('pre', {}, ansi(detail.diagnostics))));
            } else if (kind !== 'Ran') {
                const [mark, cls, said] = outcome(unit.outcome);
                lines.push(h('div', { class: cls, style: 'margin-top: 0.8em' }, `${mark} ${said}`));
            }

            // Each binding's type is what the unit's run said; what it holds
            // is what it holds now, which a later unit may have changed.
            const defined = kind === 'Ran' ? ran.map(variant) : [];
            if (detail.bindings.length > 0) {
                lines.push(h('div', { class: 'dlr-heading' }, 'Bindings'));
                for (const [name, value] of detail.bindings) {
                    const found = defined.find(([what, fields]) => what !== 'Function' && fields.name === name);
                    lines.push(h('div', {}, span('dlr-name', name),
                        found ? [': ', span('dlr-type', found[1].ty)] : null,
                        value === null ? span('dlr-dim', ' -- given away') : ` = ${value}`));
                }
            }
            const functions = defined.filter(([what]) => what === 'Function');
            if (functions.length > 0) {
                lines.push(h('div', { class: 'dlr-heading' }, 'Functions'));
                lines.push(...functions.map(([, fields]) => h('div', {}, 'fun ', span('dlr-name', fields.name))));
            }

            const accesses = ['provides', 'reads', 'takes', 'overwrites', 'updates', 'revives']
                .filter((what) => detail[what].length > 0);
            if (accesses.length > 0) {
                lines.push(h('div', { class: 'dlr-heading' }, 'Names'));
                lines.push(h('table', { class: 'dlr-env dlr-names' }, accesses.map((what) =>
                    h('tr', {}, h('td', { class: 'dlr-dim' }, what), h('td', {}, detail[what].join(', '))))));
            }
            return lines;
        }
    }

    function mount(element, trace) {
        return new Player(element, trace);
    }

    // Keep a trace by name, and play it in every element that names it.
    function register(name, trace) {
        traces[name] = trace;
        const play = () => {
            for (const element of document.querySelectorAll(`[data-replay="${name}"]`)) {
                try {
                    mount(element, trace);
                } catch (error) {
                    element.replaceChildren(h('div', { class: 'dlr-error' }, `This replay cannot be played: ${error.message}`));
                    throw error;
                }
            }
        };
        if (document.readyState === 'loading') {
            document.addEventListener('DOMContentLoaded', play);
        } else {
            play();
        }
    }

    window.DataloveReplay = { register, mount, traces };
})();
