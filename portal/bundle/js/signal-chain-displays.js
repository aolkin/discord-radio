import {
    clamp,
    controls,
    every,
    formatValue,
    fromPosition,
    hertz,
    isNumber,
    roundTo,
    toPosition,
} from './signal-chain.js';

const clamp01 = (value) => clamp(value, 0, 1);
const specs = Object.fromEntries(controls.map((spec) => [spec.id, spec]));
const left = 34;
const plotWidth = 274;

function handle(id, hot, x, y, text, anchor = 'middle', dy = -11) {
    const color = `var(${specs[id].color})`;
    return `<g class="chain-handle${hot === id ? ' hot' : ''}" data-handle="${id}"><circle class="chain-handle-hit" cx="${x}" cy="${y}" r="15"/><circle class="chain-handle-dot" cx="${x}" cy="${y}" r="6.5" fill="${color}"/><text x="${x}" y="${y + dy}" text-anchor="${anchor}" fill="${color}">${text}</text></g>`;
}

const grid = (x1, x2, y1, y2) => `<line class="chain-grid" x1="${x1}" x2="${x2}" y1="${y1}" y2="${y2}"/>`;
const caption = (x, y, text, anchor = 'middle') =>
    `<text class="chain-screen-text" x="${x}" y="${y}" text-anchor="${anchor}">${text}</text>`;
const pathOf = (points) =>
    points.map(([x, y], index) => `${index ? 'L' : 'M'}${x.toFixed(1)},${y.toFixed(1)}`).join('');

const filterX = (hz) => left + (plotWidth * Math.log(hz / 20)) / Math.log(1000);
const filterHz = (x) => 20 * 1000 ** ((x - left) / plotWidth);
const filterTop = 14;
const filterBottom = 124;
const filterY = (db) => filterTop + (filterBottom - filterTop) * clamp01(-db / 36);

function responseDb(hz, highpass, lowpass) {
    const low = (hz / highpass) ** 4;
    const high = (hz / lowpass) ** 4;
    return 10 * Math.log10(low / (1 + low)) - 10 * Math.log10(1 + high);
}

const filters = {
    height: 150,
    label: 'Frequency response. Drag the Low cut and High cut handles.',
    note: () => 'The noise passes through this module too, so the cuts shape it as well.',
    draw({ highpass, lowpass }, hot) {
        let out = [100, 1000, 10000].map((hz) => grid(filterX(hz), filterX(hz), filterTop, filterBottom)).join('');
        for (const db of [0, -12, -24, -36]) {
            out +=
                grid(left, left + plotWidth, filterY(db), filterY(db)) + caption(left - 4, filterY(db) + 3, db, 'end');
        }
        for (const [hz, text, anchor] of [
            [20, '20 Hz', 'start'],
            [100, '100'],
            [1000, '1k'],
            [10000, '10k'],
            [20000, '20k', 'end'],
        ]) {
            out += caption(filterX(hz), filterBottom + 13, text, anchor);
        }
        if (!(isNumber(highpass) && isNumber(lowpass) && highpass > 0 && lowpass > 0)) {
            return out;
        }
        const curve = [];
        for (let step = 0; step <= 160; step++) {
            const x = left + (plotWidth * step) / 160;
            curve.push([x, filterY(responseDb(filterHz(x), highpass, lowpass))]);
        }
        const area = `M${left},${filterBottom}${pathOf(curve).replace('M', 'L')}L${left + plotWidth},${filterBottom}Z`;
        const lowX = filterX(clamp(highpass, 20, 20000));
        const highX = filterX(clamp(lowpass, 20, 20000));
        const close = highX - lowX < 170;
        const lowAnchor = close ? (lowX - left > 100 ? 'end' : 'start') : lowX < left + 50 ? 'start' : 'middle';
        const highAnchor = close
            ? left + plotWidth - highX > 100
                ? 'start'
                : 'end'
            : highX > left + plotWidth - 50
              ? 'end'
              : 'middle';
        const highDy = close && (lowX - left <= 100 || left + plotWidth - highX <= 100) ? 22 : -11;
        return `${out}<path d="${area}" fill="var(--chain-indigo)" opacity="0.22"/><path d="${pathOf(curve)}" class="chain-trace"/>${handle(
            'filters.highpass',
            hot,
            lowX,
            filterY(responseDb(highpass, highpass, lowpass)),
            `Low cut ${hertz(highpass)}`,
            lowAnchor,
        )}${handle(
            'filters.lowpass',
            hot,
            highX,
            filterY(responseDb(lowpass, highpass, lowpass)),
            `High cut ${hertz(lowpass)}`,
            highAnchor,
            highDy,
        )}`;
    },
    pick: (point, { highpass, lowpass }) =>
        Math.abs(point.x - filterX(highpass)) <= Math.abs(point.x - filterX(lowpass))
            ? 'filters.highpass'
            : 'filters.lowpass',
    drag: (_id, point) => Math.round(clamp(filterHz(clamp(point.x, left, left + plotWidth)), 20, 20000)),
};

const fadeTop = 26;
const fadeBottom = 126;
const fadeWidth = 248;
const gainY = (gain) => fadeBottom - ((fadeBottom - fadeTop) * gain) / 1.3;
const yGain = (y) => ((fadeBottom - y) * 1.3) / (fadeBottom - fadeTop);
const triangle = (phase) => (phase < 0.5 ? phase * 4 - 1 : 3 - phase * 4);
const timeX = (seconds) => left + (fadeWidth * seconds) / 4;

function fadeShape(values) {
    const depth = isNumber(values.depth) ? clamp01(values.depth) : 0;
    const rate = isNumber(values.rate) ? values.rate : 0;
    const jitter = isNumber(values.jitter) ? values.jitter : 0;
    const largest = Math.min(1.5, 1 + jitter);
    const smallest = Math.max(0.5, 1 - jitter);
    return { depth, rate, jitter, largest, smallest, deepest: 1 - depth * (0.5 + 0.5 * largest) };
}

const tremolo = {
    height: 150,
    label: 'Volume over four seconds. Drag the Depth, Unevenness and Speed handles.',
    note: (values) =>
        values.depth > 0 && values.jitter > 0
            ? 'Shaded: the range of fade sizes. Dotted line: the deepest dip.'
            : 'The bot also varies the speed by up to ±70% on its own.',
    draw(values, hot) {
        const { depth, rate, jitter, largest, smallest, deepest } = fadeShape(values);
        let out = '';
        for (const gain of [0, 0.5, 1]) {
            out +=
                grid(left, left + fadeWidth, gainY(gain), gainY(gain)) +
                caption(left - 4, gainY(gain) + 3, `${gain * 100}%`, 'end');
        }
        for (let second = 0; second <= 4; second++) {
            out += caption(timeX(second), fadeBottom + 13, `${second} s`);
        }
        const line = [];
        const upper = [];
        const lower = [];
        for (let step = 0; step <= 240; step++) {
            const seconds = (4 * step) / 240;
            const wave = rate > 0 ? triangle((seconds * rate) % 1) : -1;
            const gain = (scale) => 1 - depth + (wave * scale * 0.5 + 0.5) * depth;
            const x = timeX(seconds);
            line.push([x, gainY(gain(1))]);
            upper.push([x, gainY(Math.max(gain(smallest), gain(largest)))]);
            lower.unshift([x, gainY(Math.min(gain(smallest), gain(largest)))]);
        }
        const band =
            depth > 0 && jitter > 0
                ? `<path d="${pathOf(upper)}${pathOf(lower).replace('M', 'L')}Z" fill="var(--chain-rose)" opacity="0.22"/>`
                : '';
        const right = left + fadeWidth + 14;
        const period = rate > 0 ? 1 / rate : null;
        const bracketX = period === null ? timeX(4) : timeX(Math.min(period, 4));
        let speed = `Speed: one fade per ${every(rate)}`;
        if (period === null) {
            speed = 'Speed: frozen';
        } else if (period > 4) {
            speed = `Speed: one fade every ${every(rate)} →`;
        }
        const depthY = gainY(1 - depth);
        const gap = Math.abs(gainY(deepest) - depthY);
        const deepLine =
            depth > 0 && jitter > 0
                ? `<line x1="${left}" x2="${right}" y1="${gainY(deepest)}" y2="${gainY(deepest)}" stroke="var(--chain-rose)" stroke-dasharray="1.5 2.5"/>`
                : '';
        return `${out}${band}<line x1="${left}" x2="${right}" y1="${depthY}" y2="${depthY}" stroke="var(--chain-indigo)" stroke-dasharray="4 3"/>${deepLine}<path d="${pathOf(line)}" class="chain-trace"/><path d="M${left},10 V16 H${bracketX} V10" stroke="var(--chain-amber)" stroke-width="1.4" fill="none"/>${handle(
            'tremolo.rate',
            hot,
            bracketX,
            16,
            speed,
            bracketX > left + fadeWidth - 60 ? 'end' : 'start',
            -4.5,
        )}${handle('tremolo.depth', hot, right, depthY, 'Depth', 'end', -9)}${handle(
            'tremolo.jitter',
            hot,
            right,
            gainY(deepest) + (gap < 16 ? 16 - gap : 0),
            'Unevenness',
            'end',
            18,
        )}`;
    },
    pick: () => null,
    drag(id, point, values) {
        if (id === 'tremolo.depth') {
            return roundTo(clamp01(1 - yGain(point.y)), 2);
        }
        if (id === 'tremolo.jitter') {
            if (!(values.depth > 0)) {
                return values.jitter;
            }
            const largest = ((1 - yGain(point.y)) / values.depth - 0.5) / 0.5;
            return roundTo(clamp(largest - 1, 0, 0.5), 2);
        }
        const seconds = ((point.x - left) / fadeWidth) * 4;
        return roundTo(clamp(1 / Math.max(seconds, 0.05), 0.25, 20), 2);
    },
};

const tone = 0.9;
const clipTop = 12;
const clipBottom = 128;
const clipMiddle = (clipTop + clipBottom) / 2;
const clipWidth = 250;
const clipLimit = 2.2;
const levelY = (level) => clipMiddle - (level / clipLimit) * ((clipBottom - clipTop) / 2);
const yLevel = (y) => ((clipMiddle - y) / ((clipBottom - clipTop) / 2)) * clipLimit;

function softClip(sample, threshold) {
    if (threshold <= 0) {
        return 0;
    }
    if (Math.abs(sample) < threshold) {
        return sample;
    }
    return threshold * Math.sign(sample) * (1 - Math.exp(-Math.abs(sample) / threshold));
}

const clipInputs = (values) => ({
    pregain: isNumber(values.pregain) ? values.pregain : 1,
    threshold: isNumber(values.threshold) ? values.threshold : 1,
});

const clip = {
    height: 150,
    label: 'A test tone before and after clipping. Drag the Drive and Clip level handles.',
    note(values) {
        const { pregain, threshold } = clipInputs(values);
        const peak = tone * pregain;
        const out = Math.abs(softClip(peak, threshold));
        const squashed =
            peak > out + 0.005 ? `squashed from ${roundTo(peak, 2)} to ${roundTo(out, 2)}` : 'not squashed';
        return `Dashed: test tone in. Thin: after Drive. Solid: out. Peak ${squashed}.`;
    },
    draw(values, hot) {
        const { pregain, threshold } = clipInputs(values);
        let out = '';
        for (const level of [-2, -1, 0, 1, 2]) {
            out +=
                grid(left, left + clipWidth, levelY(level), levelY(level)) +
                caption(left - 4, levelY(level) + 3, level, 'end');
        }
        out += caption(left + clipWidth, levelY(-1) + 10, '±1 = full scale', 'end');
        const input = [];
        const driven = [];
        const output = [];
        for (let step = 0; step <= 300; step++) {
            const sample = tone * Math.sin((4 * Math.PI * step) / 300);
            const x = left + (clipWidth * step) / 300;
            input.push([x, levelY(sample)]);
            driven.push([x, levelY(clamp(sample * pregain, -clipLimit, clipLimit))]);
            output.push([x, levelY(clamp(softClip(sample * pregain, threshold), -clipLimit, clipLimit))]);
        }
        const level = clamp(threshold, 0, 2);
        const right = left + clipWidth + 14;
        const peak = tone * pregain;
        const peakX = left + clipWidth / 8;
        const peakY = levelY(Math.min(peak, clipLimit));
        return `${out}<line x1="${left}" x2="${right}" y1="${levelY(level)}" y2="${levelY(level)}" stroke="var(--chain-teal)" stroke-dasharray="4 3"/><line x1="${left}" x2="${left + clipWidth}" y1="${levelY(-level)}" y2="${levelY(-level)}" stroke="var(--chain-teal)" stroke-dasharray="4 3"/><path d="${pathOf(input)}" class="chain-ghost"/><path d="${pathOf(driven)}" stroke="var(--chain-amber)" stroke-width="1.2" fill="none" opacity="0.8"/><path d="${pathOf(output)}" class="chain-trace"/>${handle(
            'clip.pregain',
            hot,
            peakX,
            peakY,
            `Drive ×${roundTo(pregain, 2)}${peak > clipLimit ? ' ↑' : ''}`,
            'start',
            peakY < 24 ? 18 : -10,
        )}${handle('clip.threshold', hot, right, levelY(level), 'Clip level', 'end', levelY(level) < 24 ? 18 : -9)}`;
    },
    pick: () => null,
    drag(id, point, _values, start) {
        if (id === 'clip.threshold') {
            return roundTo(clamp(Math.abs(yLevel(point.y)), 0.01, 2), 2);
        }
        const from = isNumber(start.value) && start.value > 0 ? start.value : 1;
        return roundTo(clamp(from * 2 ** ((start.y - point.y) / 25), 0.1, 100), 2);
    },
};

const bitcrush = {
    height: 120,
    label: 'One wave cycle after bit depth rounding.',
    note: () => '',
    draw({ bits }) {
        const middle = 57;
        const sampleY = (sample) => middle - sample * 47 * 0.95;
        const step = isNumber(bits) ? 1 / 2 ** (bits - 1) : 0;
        const smooth = [];
        const rounded = [];
        for (let index = 0; index <= 500; index++) {
            const x = left + (plotWidth * index) / 500;
            const sample = 0.9 * Math.sin((2 * Math.PI * index) / 500);
            smooth.push([x, sampleY(sample)]);
            rounded.push([x, sampleY(step ? Math.round(sample / step) * step : sample)]);
        }
        let text = `${bits} bits: ${2 ** bits} steps from −1 to 1`;
        if (!isNumber(bits)) {
            text = 'Off: samples pass unchanged';
        } else if (bits > 6) {
            text = `${bits} bits: steps too fine to draw at this size`;
        }
        return `${grid(left, left + plotWidth, middle, middle)}<path d="${pathOf(smooth)}" class="chain-ghost"/><path d="${pathOf(rounded)}" stroke="var(--chain-violet)" stroke-width="2" fill="none"/>${caption(left + plotWidth / 2, 116, text)}`;
    },
    pick: () => null,
};

function random(seed) {
    let state = seed >>> 0;
    return () => {
        state = (state + 0x6d2b79f5) >>> 0;
        let mixed = state;
        mixed = Math.imul(mixed ^ (mixed >>> 15), mixed | 1);
        mixed ^= mixed + Math.imul(mixed ^ (mixed >>> 7), mixed | 61);
        return ((mixed ^ (mixed >>> 14)) >>> 0) / 4294967296;
    };
}

const dropTop = 16;
const dropBottom = 60;
const rulerY = 112;
const rulerX = (id, ms) => left + plotWidth * toPosition(specs[id], ms);

function dropInputs(values) {
    const shortest = isNumber(values.min_ms) ? values.min_ms : 0;
    return {
        probability: isNumber(values.probability) ? values.probability : 0,
        shortest,
        longest: isNumber(values.max_ms) ? Math.max(values.max_ms, shortest) : shortest,
    };
}

function exampleDropouts(values) {
    const { probability, shortest, longest } = dropInputs(values);
    const next = random(7);
    const list = [];
    for (let ms = 0; ms < 20000; ms += 100) {
        if (next() < probability / 10) {
            const length = shortest + next() * (longest - shortest);
            list.push([ms, length]);
            ms += length;
        }
    }
    return list;
}

const dropout = {
    height: 150,
    label: 'A 20 second example of dropouts, and a ruler with Shortest and Longest handles.',
    note: (values) =>
        values.probability > 0
            ? `${exampleDropouts(values).length} dropouts in this example; about ${Math.round(values.probability * 20)} on average.`
            : 'Dropouts are off.',
    draw(values, hot) {
        const { shortest, longest } = dropInputs(values);
        const timeAt = (ms) => left + (plotWidth * ms) / 20000;
        let out = `<rect x="${left}" y="${dropTop}" width="${plotWidth}" height="${dropBottom - dropTop}" rx="3" fill="var(--chain-indigo)" opacity="0.45"/>`;
        for (const [ms, length] of exampleDropouts(values)) {
            out += `<rect x="${timeAt(ms).toFixed(1)}" y="${dropTop - 4}" width="${Math.max((plotWidth * length) / 20000, 1.4).toFixed(2)}" height="${dropBottom - dropTop + 8}" fill="var(--chain-silence)"/>`;
        }
        for (let second = 0; second <= 20; second += 5) {
            out += caption(
                timeAt(second * 1000),
                dropBottom + 12,
                `${second} s`,
                second === 0 ? 'start' : second === 20 ? 'end' : 'middle',
            );
        }
        out +=
            caption(left, dropTop - 6, 'signal', 'start') +
            caption(left + plotWidth, dropTop - 6, 'red: silence', 'end');
        out += `<line class="chain-grid chain-ruler" x1="${left}" x2="${left + plotWidth}" y1="${rulerY}" y2="${rulerY}"/>`;
        for (const [ms, text] of [
            [0, 'off'],
            [10, '10'],
            [100, '100'],
            [1000, '1000'],
        ]) {
            out += caption(rulerX('dropout.min_ms', ms), rulerY + 18, text);
        }
        const from = rulerX('dropout.min_ms', shortest);
        const to = rulerX('dropout.max_ms', longest);
        const close = to - from < 60;
        out += `<line x1="${from}" x2="${to}" y1="${rulerY}" y2="${rulerY}" stroke="var(--chain-indigo)" stroke-width="4"/>`;
        out += caption(left - 4, rulerY + 3, 'ms', 'end');
        out += handle(
            'dropout.min_ms',
            hot,
            from,
            rulerY,
            `Shortest ${formatValue(specs['dropout.min_ms'], shortest)}`,
            close ? 'end' : 'middle',
        );
        out += handle(
            'dropout.max_ms',
            hot,
            to,
            rulerY,
            `Longest ${formatValue(specs['dropout.max_ms'], longest)}`,
            close ? 'start' : to > left + plotWidth - 40 ? 'end' : 'middle',
        );
        return out;
    },
    pick(point, values) {
        if (point.y <= rulerY - 20) {
            return null;
        }
        const { shortest, longest } = dropInputs(values);
        return Math.abs(point.x - rulerX('dropout.min_ms', shortest)) <=
            Math.abs(point.x - rulerX('dropout.max_ms', longest))
            ? 'dropout.min_ms'
            : 'dropout.max_ms';
    },
    drag: (id, point) => fromPosition(specs[id], (point.x - left) / plotWidth),
};

export const displays = { filters, tremolo, clip, bitcrush, dropout };
