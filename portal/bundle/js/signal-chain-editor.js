import {
    clamp,
    clone,
    formatValue,
    formErrors,
    fromPosition,
    isNumber,
    keepOrdered,
    kindOf,
    kinds,
    limitText,
    maxEffects,
    newEffect,
    noiseColors,
    roundTo,
    slot,
    toPosition,
} from './signal-chain.js';

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
        kinds,
        noiseColors,
        maxEffects,
        addKind: kinds[0].kind,
        hot: null,
        typing: null,
        typed: '',
        drag: null,

        init() {
            this.$watch('chain.name', () => {
                this.typing = null;
                this.drag = null;
                this.hot = null;
            });
        },

        form() {
            return this.chain.draft.form;
        },

        kindOf,
        slot,

        value(effect, spec) {
            return effect.params[spec.key];
        },

        savedValue(effect, spec) {
            const saved = this.chain.draft.saved.effects.find(
                (entry) => entry.name === effect.name && entry.kind === effect.kind,
            );
            return saved?.params[spec.key];
        },

        setValue(effect, spec, value) {
            effect.params[spec.key] = clamp(keepOrdered(effect.params, spec, value), spec.min, spec.max);
        },

        canAdd() {
            return this.form().effects.length < maxEffects;
        },

        add() {
            if (this.canAdd()) {
                const effects = this.form().effects;
                effects.push(newEffect(this.addKind, effects));
            }
        },

        remove(index) {
            this.form().effects.splice(index, 1);
        },

        dirty() {
            return JSON.stringify(this.form().effects) !== JSON.stringify(this.chain.draft.saved.effects);
        },

        discard() {
            this.form().effects = clone(this.chain.draft.saved.effects);
        },

        errors() {
            return formErrors(this.form());
        },

        isBad(index, spec) {
            const id = slot(index, spec.key);
            return this.errors().some((error) => error.ids.includes(id));
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
            return this.chain.isDefault(this.chain.name) || !this.dirty();
        },

        async save() {
            if (!this.saveDisabled()) {
                await this.chain.save();
            }
        },

        step(direction) {
            const names = this.chain.names;
            const index = names.indexOf(this.chain.name);
            this.chain.open(names[(index + direction + names.length) % names.length]);
        },

        knobSvg(effect, spec) {
            const position = toPosition(spec, this.value(effect, spec));
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

        meaning(effect, spec) {
            const value = this.value(effect, spec);
            return isNumber(value) ? spec.meaning(value) : '';
        },

        valueText(effect, spec) {
            const meaning = this.meaning(effect, spec);
            const text = formatValue(spec, this.value(effect, spec));
            return meaning ? `${text}, ${meaning}` : text;
        },

        formatValue,
        limitText,
        toPosition,

        startDrag(event, effect, index, spec) {
            if (event.button !== 0) {
                return;
            }
            event.preventDefault();
            const target = event.currentTarget;
            target.setPointerCapture(event.pointerId);
            target.focus({ preventScroll: true });
            const fader = kindOf(effect).faders === true;
            this.drag = {
                effect,
                spec,
                fader,
                target,
                x: event.clientX,
                y: event.clientY,
                position: toPosition(spec, this.value(effect, spec)),
            };
            this.hot = slot(index, spec.key);
            if (fader) {
                this.moveFader(event);
            }
        },

        moveFader(event) {
            const rect = this.drag.target.getBoundingClientRect();
            const position = (rect.bottom - 6 - event.clientY) / (rect.height - 12);
            this.setValue(this.drag.effect, this.drag.spec, fromPosition(this.drag.spec, position));
        },

        moveDrag(event) {
            if (!this.drag) {
                return;
            }
            if (this.drag.fader) {
                this.moveFader(event);
                return;
            }
            const fine = event.shiftKey ? 0.1 : 1;
            const moved = (this.drag.y - event.clientY + (event.clientX - this.drag.x) * 0.5) / 180;
            this.setValue(
                this.drag.effect,
                this.drag.spec,
                fromPosition(this.drag.spec, this.drag.position + moved * fine),
            );
        },

        endDrag() {
            this.drag = null;
            this.hot = null;
        },

        nudge(effect, spec, direction, big) {
            const value = this.value(effect, spec);
            let next = fromPosition(spec, toPosition(spec, value) + direction * (big ? 0.1 : 0.01));
            if (same(next, value)) {
                const current = isNumber(value) ? value : 0;
                next = roundTo(clamp(current + direction * 10 ** -spec.decimals, spec.min, spec.max), spec.decimals);
            }
            this.setValue(effect, spec, next);
        },

        wheel(event, effect, spec) {
            this.nudge(effect, spec, event.deltaY < 0 ? 1 : -1, false);
        },

        resetToSaved(effect, spec) {
            const saved = this.savedValue(effect, spec);
            if (saved !== undefined) {
                effect.params[spec.key] = saved;
            }
        },

        sliderKey(event, effect, index, spec) {
            if (event.key in nudgeKeys) {
                event.preventDefault();
                this.nudge(effect, spec, nudgeKeys[event.key], event.key.startsWith('Page'));
            } else if (event.key === 'Home' || event.key === 'End') {
                event.preventDefault();
                this.setValue(effect, spec, event.key === 'Home' ? spec.min : spec.max);
            } else if (event.key === 'Enter') {
                event.preventDefault();
                this.startTyping(effect, index, spec);
            }
        },

        startTyping(effect, index, spec) {
            this.typed = String(this.value(effect, spec));
            this.typing = slot(index, spec.key);
        },

        finishTyping(effect, index, spec, commit) {
            if (this.typing !== slot(index, spec.key)) {
                return;
            }
            this.typing = null;
            if (commit) {
                const raw = this.typed.trim();
                const value = Number(raw);
                effect.params[spec.key] = raw === '' || Number.isNaN(value) ? '' : value;
            }
        },
    };
}

document.addEventListener('alpine:init', () => {
    Alpine.data('signalChainEditor', signalChainEditor);
});
