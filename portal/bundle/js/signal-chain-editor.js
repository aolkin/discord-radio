import {
    clamp,
    controls,
    formatValue,
    formErrors,
    fromPosition,
    isNumber,
    keepOrdered,
    limitText,
    roundTo,
    effects,
    toPosition,
} from './signal-chain.js';

const effectIds = effects.map((effect) => effect.id);
const snapshot = (form) => JSON.stringify(Object.fromEntries(effectIds.map((id) => [id, form[id]])));
const same = (a, b) => (isNumber(a) && isNumber(b) ? Math.abs(a - b) < 1e-9 : a === b);
const nudgeKeys = { ArrowUp: 1, ArrowRight: 1, ArrowDown: -1, ArrowLeft: -1, PageUp: 1, PageDown: -1 };
const knobStart = -135;
const knobEnd = 135;
const faderTicks = [
    [2, '+18'],
    [1, '0 dB'],
    [0.5, '−30'],
    [0, 'off'],
];

function point(angle, radius) {
    const radians = (angle * Math.PI) / 180;
    return [30 + radius * Math.sin(radians), 30 - radius * Math.cos(radians)];
}

function arc(from, to, radius) {
    const [x0, y0] = point(from, radius);
    const [x1, y1] = point(to, radius);
    return `M${x0.toFixed(2)},${y0.toFixed(2)} A${radius},${radius} 0 ${to - from > 180 ? 1 : 0} 1 ${x1.toFixed(2)},${y1.toFixed(2)}`;
}

export function signalChainEditor(chain) {
    return {
        chain,
        effects,
        hot: null,
        panel: null,
        saveName: '',
        collapsed: {},
        typing: null,
        typed: '',
        drag: null,
        wheelAt: 0,

        init() {
            this.$watch('chain.name', () => {
                this.panel = null;
                this.typing = null;
                this.drag = null;
                this.hot = null;
            });
        },

        form() {
            return this.chain.draft.form;
        },

        history() {
            this.chain.draft.history ??= { undo: [], redo: [], memory: {} };
            return this.chain.draft.history;
        },

        canUndo() {
            return (this.chain.draft?.history?.undo.length ?? 0) > 0;
        },

        canRedo() {
            return (this.chain.draft?.history?.redo.length ?? 0) > 0;
        },

        value(spec) {
            return this.form()[spec.effect][spec.key];
        },

        savedValue(spec) {
            return this.chain.draft.saved[spec.effect][spec.key];
        },

        restore(text) {
            const values = JSON.parse(text);
            for (const id of effectIds) {
                this.form()[id] = values[id];
            }
        },

        record(before) {
            if (before !== snapshot(this.form())) {
                const history = this.history();
                history.undo.push(before);
                history.redo = [];
            }
        },

        change(apply) {
            const before = snapshot(this.form());
            apply();
            this.record(before);
        },

        undo() {
            const history = this.history();
            if (history.undo.length > 0) {
                history.redo.push(snapshot(this.form()));
                this.restore(history.undo.pop());
            }
        },

        redo() {
            const history = this.history();
            if (history.redo.length > 0) {
                history.undo.push(snapshot(this.form()));
                this.restore(history.redo.pop());
            }
        },

        discard() {
            this.change(() => this.restore(snapshot(this.chain.draft.saved)));
        },

        setValue(spec, value) {
            const ordered = keepOrdered(this.form(), spec, value);
            this.form()[spec.effect][spec.key] = clamp(ordered, spec.min, spec.max);
        },

        effectValues(effect) {
            return this.form()[effect.id];
        },

        isOn(effect) {
            return effect.isOn(this.effectValues(effect));
        },

        power(effect) {
            this.change(() => {
                const values = this.effectValues(effect);
                const memory = this.history().memory;
                if (effect.isOn(values)) {
                    memory[effect.id] = Object.fromEntries(Object.keys(effect.off).map((key) => [key, values[key]]));
                    Object.assign(values, effect.off);
                } else if (memory[effect.id] && effect.isOn({ ...values, ...memory[effect.id] })) {
                    Object.assign(values, memory[effect.id]);
                } else {
                    Object.assign(values, effect.on);
                }
                if (effect.isOn(values)) {
                    effect.afterOn?.(values);
                }
            });
        },

        toggleCollapsed(effect) {
            this.collapsed[effect.id] = !this.collapsed[effect.id];
        },

        setAllCollapsed(collapsed) {
            this.collapsed = Object.fromEntries(effectIds.map((id) => [id, collapsed]));
        },

        jump(effect) {
            this.collapsed[effect.id] = false;
            this.$nextTick(() => {
                const module = document.getElementById(`chain-module-${effect.id}`);
                const reduce = matchMedia('(prefers-reduced-motion: reduce)').matches;
                module.scrollIntoView({ behavior: reduce ? 'auto' : 'smooth', block: 'nearest' });
                module.querySelector('.chain-power').focus({ preventScroll: true });
            });
        },

        errors() {
            return formErrors(this.form());
        },

        isBad(spec) {
            return this.errors().some((error) => error.ids.includes(spec.id));
        },

        changes() {
            return controls.filter((spec) => !same(this.value(spec), this.savedValue(spec)));
        },

        changeText() {
            const count = this.changes().length;
            if (count === 0) {
                return 'No unsaved changes';
            }
            return count === 1 ? '1 unsaved change' : `${count} unsaved changes`;
        },

        nameError() {
            const name = this.form().name;
            if (!/^[\w-]+$/.test(name)) {
                return 'Use only letters, digits, - and _.';
            }
            return this.chain.names.includes(name) ? `A signal preset named ${name} already exists.` : '';
        },

        saveDisabled() {
            if (this.chain.saving || this.errors().length > 0) {
                return true;
            }
            if (this.chain.isNew) {
                return this.nameError() !== '';
            }
            return this.chain.isDefault(this.chain.name) || this.changes().length === 0;
        },

        async save() {
            if (this.saveDisabled()) {
                return;
            }
            if (this.chain.isNew) {
                await this.chain.save();
            } else {
                this.panel = 'review';
                this.$nextTick(() => this.$root.querySelector('.chain-review-apply')?.focus());
            }
        },

        async applySave() {
            await this.chain.save();
            if (this.changes().length === 0) {
                this.panel = null;
            }
        },

        openSaveAs() {
            const base = `${this.chain.name}_copy`;
            let name = base;
            for (let count = 2; this.chain.names.includes(name); count++) {
                name = `${base}${count}`;
            }
            this.saveName = name;
            this.panel = 'saveas';
            this.$nextTick(() => this.$root.querySelector('.chain-saveas-name')?.select());
        },

        saveAsError() {
            if (!/^[\w-]+$/.test(this.saveName)) {
                return 'Use only letters, digits, - and _.';
            }
            if (this.chain.names.includes(this.saveName)) {
                return `A signal preset named ${this.saveName} already exists.`;
            }
            return this.errors().length > 0 ? 'Fix the marked values first.' : '';
        },

        async applySaveAs() {
            if (this.saveAsError() === '' && !this.chain.saving) {
                await this.chain.saveAs(this.saveName);
            }
        },

        step(direction) {
            const names = this.chain.names;
            const index = names.indexOf(this.chain.name);
            this.chain.open(names[(index + direction + names.length) % names.length]);
        },

        shortcut(event) {
            if (
                !this.chain.active ||
                !this.chain.draft ||
                !(event.ctrlKey || event.metaKey) ||
                event.key.toLowerCase() !== 'z' ||
                event.target.closest('input, select, textarea')
            ) {
                return;
            }
            event.preventDefault();
            if (event.shiftKey) {
                this.redo();
            } else {
                this.undo();
            }
        },

        knobSvg(spec) {
            const position = toPosition(spec, this.value(spec));
            const angle = knobStart + (knobEnd - knobStart) * position;
            const color = `var(${spec.color})`;
            const [innerX, innerY] = point(angle, 15);
            const [outerX, outerY] = point(angle, 6);
            const filled =
                position > 0.002
                    ? `<path class="chain-knob-value" stroke="${color}" d="${arc(knobStart, angle, 26)}"/>`
                    : '';
            return `<path class="chain-knob-track" d="${arc(knobStart, knobEnd, 26)}"/>${filled}<circle class="chain-knob-face" cx="30" cy="30" r="19"/><line class="chain-knob-pointer" stroke="${color}" x1="${outerX.toFixed(2)}" y1="${outerY.toFixed(2)}" x2="${innerX.toFixed(2)}" y2="${innerY.toFixed(2)}"/>`;
        },

        faderOffset(spec, value) {
            return `calc(6px + (100% - 12px) * ${toPosition(spec, value)})`;
        },

        faderTicks,

        valueText(spec) {
            const value = this.value(spec);
            const meaning = isNumber(value) || value === null ? spec.meaning(value) : '';
            return meaning ? `${formatValue(spec, value)}, ${meaning}` : formatValue(spec, value);
        },

        meaning(spec) {
            const value = this.value(spec);
            return isNumber(value) || value === null ? spec.meaning(value) : '';
        },

        formatValue,
        limitText,
        toPosition,

        startDrag(event, spec, kind) {
            if (event.button !== 0) {
                return;
            }
            event.preventDefault();
            const target = event.currentTarget;
            target.setPointerCapture(event.pointerId);
            target.focus({ preventScroll: true });
            this.drag = {
                spec,
                kind,
                target,
                before: snapshot(this.form()),
                x: event.clientX,
                y: event.clientY,
                position: toPosition(spec, this.value(spec)),
            };
            this.hot = spec.id;
            if (kind === 'fader') {
                this.moveFader(event);
            }
        },

        moveFader(event) {
            const rect = this.drag.target.getBoundingClientRect();
            const position = (rect.bottom - 6 - event.clientY) / (rect.height - 12);
            this.setValue(this.drag.spec, fromPosition(this.drag.spec, position));
        },

        moveDrag(event) {
            if (!this.drag) {
                return;
            }
            if (this.drag.kind === 'fader') {
                this.moveFader(event);
                return;
            }
            const fine = event.shiftKey ? 0.1 : 1;
            const moved = (this.drag.y - event.clientY + (event.clientX - this.drag.x) * 0.5) / 180;
            this.setValue(this.drag.spec, fromPosition(this.drag.spec, this.drag.position + moved * fine));
        },

        endDrag() {
            if (this.drag) {
                this.record(this.drag.before);
                this.drag = null;
                this.hot = null;
            }
        },

        nudge(spec, direction, big) {
            const value = this.value(spec);
            if (spec.nullable) {
                this.form()[spec.effect][spec.key] = value === null ? 8 : clamp(value + direction, spec.min, spec.max);
                return;
            }
            let next = fromPosition(spec, toPosition(spec, value) + direction * (big ? 0.1 : 0.01));
            if (same(next, value)) {
                const current = isNumber(value) ? value : 0;
                next = roundTo(clamp(current + direction * 10 ** -spec.decimals, spec.min, spec.max), spec.decimals);
            }
            this.setValue(spec, next);
        },

        wheel(event, spec) {
            const now = Date.now();
            const before = snapshot(this.form());
            const grouped = now - this.wheelAt < 500;
            this.wheelAt = now;
            this.nudge(spec, event.deltaY < 0 ? 1 : -1, false);
            if (!grouped) {
                this.record(before);
            }
        },

        resetToSaved(spec) {
            this.change(() => {
                this.form()[spec.effect][spec.key] = this.savedValue(spec);
            });
        },

        sliderKey(event, spec) {
            if (event.key in nudgeKeys) {
                event.preventDefault();
                this.change(() => this.nudge(spec, nudgeKeys[event.key], event.key.startsWith('Page')));
            } else if (event.key === 'Home' || event.key === 'End') {
                event.preventDefault();
                this.change(() => this.setValue(spec, event.key === 'Home' ? spec.min : spec.max));
            } else if (event.key === 'Enter') {
                event.preventDefault();
                this.startTyping(spec);
            }
        },

        startTyping(spec) {
            const value = this.value(spec);
            this.typed = value === null ? '' : String(value);
            this.typing = spec.id;
        },

        finishTyping(spec, commit) {
            if (this.typing !== spec.id) {
                return;
            }
            this.typing = null;
            if (commit) {
                const raw = this.typed.trim();
                let value = raw === '' ? '' : Number(raw);
                if (raw === '' || raw.toLowerCase() === 'off') {
                    value = spec.nullable ? null : '';
                } else if (Number.isNaN(value)) {
                    value = '';
                }
                this.change(() => {
                    this.form()[spec.effect][spec.key] = value;
                });
            }
        },
    };
}

document.addEventListener('alpine:init', () => {
    Alpine.data('signalChainEditor', signalChainEditor);
});
