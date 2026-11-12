import assert from 'node:assert/strict';
import { test } from 'node:test';
import { rebalance, setAlone, shareOf } from '../../static/js/weights.js';

const total = (weights) => weights.reduce((sum, weight) => sum + (typeof weight === 'number' ? weight : 0), 0);

test('rebalance keeps whole weights, a fixed total, and pinned shares', () => {
    const start = [20, 5, 2, 8, 2];
    const pinned = [false, true, false, true, false];
    const result = rebalance(start, 0, 50, pinned);
    assert.ok(result.every(Number.isInteger));
    assert.equal(total(result), 111);
    for (const i of [1, 3]) {
        assert.equal(shareOf(result, i), shareOf(start, i));
    }
    assert.deepEqual(rebalance([60, 15, 6], 0, 90, [false, true, true]), [60, 15, 6]);
    assert.deepEqual(rebalance([5], 0, 40, []), [5]);
    assert.deepEqual(rebalance([50, 0, 50], 0, 20, []), [20, 0, 80]);
    assert.deepEqual(rebalance([50, 0, 0], 0, 20, []), [20, 40, 40]);
    assert.deepEqual(rebalance([60, '', 40], 0, 30, [false, true, false]), [30, '', 70]);
});

test('setAlone changes only the dragged weight', () => {
    const start = [20, 5, 2, 8, 2];
    const result = setAlone(start, 1, 50);
    assert.deepEqual(result, [20, 32, 2, 8, 2]);
    assert.deepEqual(setAlone([3, '', 0], 0, 40), [3, '', 0]);
    assert.deepEqual(setAlone([10, '', 30], 1, 20), [10, 10, 30]);
    assert.equal(setAlone([1, 1], 0, 100)[1], 1);
});
