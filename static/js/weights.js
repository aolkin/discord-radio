const asWeight = (value) => (typeof value === 'number' && value > 0 ? value : 0);

const sum = (values) => values.reduce((total, value) => total + value, 0);

export function scaleTo100(weights) {
    const values = weights.map(asWeight);
    const total = sum(values);
    const factor = total > 0 && total < 100 ? Math.ceil(100 / total) : 1;
    return values.map((value) => Math.round(value * factor));
}

function distribute(amount, parts) {
    const total = sum(parts);
    const exact = parts.map((part) => (total > 0 ? (part / total) * amount : amount / parts.length));
    const result = exact.map(Math.floor);
    const byRemainder = exact.map((_, i) => i).sort((a, b) => exact[b] - result[b] - (exact[a] - result[a]));
    for (const i of byRemainder.slice(0, amount - sum(result))) {
        result[i] += 1;
    }
    return result;
}

const keepEqual = (weights, result) => result.map((value, i) => (asWeight(weights[i]) === value ? weights[i] : value));

export function rebalance(weights, index, share, pinned) {
    const scaled = scaleTo100(weights);
    const total = sum(scaled);
    const free = scaled.map((_, i) => i).filter((i) => i !== index && !pinned[i]);
    if (total === 0 || free.length === 0) {
        return weights;
    }
    const room = scaled[index] + sum(free.map((i) => scaled[i]));
    const own = Math.min(room, Math.max(0, Math.round((share / 100) * total)));
    const rest = distribute(
        room - own,
        free.map((i) => scaled[i]),
    );
    const result = [...scaled];
    result[index] = own;
    free.forEach((i, k) => {
        result[i] = rest[k];
    });
    return keepEqual(weights, result);
}

export function setAlone(weights, index, share) {
    const others = sum(weights.map(asWeight)) - asWeight(weights[index]);
    if (others === 0) {
        return weights;
    }
    const fraction = Math.min(Math.max(share, 0), 99) / 100;
    const result = [...weights];
    result[index] = Math.round((fraction * others) / (1 - fraction));
    return keepEqual(weights, result);
}

export function shareOf(weights, index) {
    const total = sum(weights.map(asWeight));
    return total > 0 ? (asWeight(weights[index]) / total) * 100 : 0;
}

const sameRow = (a, b) => Alpine.raw(a) === Alpine.raw(b);

export function weightSliders() {
    return {
        weightModes: [],
        weightDrag: null,

        weightShare(rows, index) {
            return shareOf(
                rows.map((row) => row.weight),
                index,
            );
        },

        weightShareText(rows, index) {
            return `${this.weightShare(rows, index).toFixed(1)}%`;
        },

        weightMode(rows, index) {
            return this.weightModes.find((entry) => sameRow(entry.row, rows[index]))?.mode ?? null;
        },

        toggleWeightMode(rows, index, mode) {
            const current = this.weightMode(rows, index);
            const kept = this.weightModes.filter(
                (entry) => !sameRow(entry.row, rows[index]) && rows.some((row) => sameRow(entry.row, row)),
            );
            this.weightModes = current === mode ? kept : [...kept, { row: rows[index], mode }];
        },

        endWeightDrag() {
            this.weightDrag = null;
        },

        slideWeight(rows, index, event, min = 0) {
            if (
                !this.weightDrag ||
                !sameRow(this.weightDrag.row, rows[index]) ||
                this.weightDrag.start.length !== rows.length
            ) {
                this.weightDrag = { row: rows[index], start: rows.map((row) => row.weight) };
            }
            const weights = this.weightDrag.start;
            const share = Number(event.target.value);
            const next =
                this.weightMode(rows, index) === 'alone'
                    ? setAlone(weights, index, share)
                    : rebalance(
                          weights,
                          index,
                          share,
                          rows.map((_, i) => this.weightMode(rows, i) === 'pin'),
                      );
            rows.forEach((row, i) => {
                const value = typeof next[i] === 'number' ? Math.max(min, next[i]) : next[i];
                if (row.weight !== value) {
                    row.weight = value;
                }
            });
            event.target.value = this.weightShare(rows, index);
        },
    };
}
