import { apiRequest } from './api.js';
import { blankRow, convertObject, fieldFromServer, fieldToServer } from './form-fields.js';
import { registryEditor } from './registry-editor.js';

export const signalPresetForms = () => ({
    signalPresetSetting: 'manual',
    signalPresetName: '',
    signalPresetFade: null,
    signalPresetDuration: null,
});

const linear = (max) => ({ scale: 'linear', min: 0, max, step: 0.01 });
const log = (min, max, step) => ({ scale: 'log', min, max, step });
const limited = (key, label, min, max, extra) => ({ key, label, type: 'number', step: 'any', min, max, ...extra });

const presetFields = [
    limited('bandpass_low', 'Bandpass Low (Hz)', 20, 20000, { default: 20, slider: log(20, 20000, 1) }),
    limited('bandpass_high', 'Bandpass High (Hz)', 20, 20000, { default: 20000, slider: log(20, 20000, 1) }),
    limited('white_noise_level', 'White Noise Level (0-2)', 0, 2, { slider: linear(2) }),
    limited('pink_noise_level', 'Pink Noise Level (0-2)', 0, 2, { slider: linear(2) }),
    limited('brown_noise_level', 'Brown Noise Level (0-2)', 0, 2, { slider: linear(2) }),
    limited('tremolo_depth', 'Tremolo Depth', 0, 1, { slider: linear(1) }),
    limited('tremolo_rate', 'Tremolo Rate (Hz)', 0, 20, { slider: log(0.1, 20, 0.01) }),
    limited('tremolo_jitter', 'Tremolo Jitter', 0, 1, { slider: linear(1) }),
    limited('clip_pregain', 'Clip Pregain', 0, 100, { default: 1 }),
    limited('clip_threshold', 'Clip Threshold', 0, 2, { default: 1, slider: linear(2) }),
    { key: 'bitcrush_bits', label: 'Bitcrush Bits', type: 'number', step: 1, min: 1, max: 16, optional: true },
    limited('dropout_probability', 'Dropout Probability', 0, 1, { slider: linear(1) }),
];

const dropoutFields = [
    limited(0, 'Dropout Min Duration (ms)', 0, 2000, { slider: log(1, 2000, 1) }),
    limited(1, 'Dropout Max Duration (ms)', 0, 2000, { slider: log(1, 2000, 1) }),
];

function blocks(form) {
    return [
        { row: form, fields: presetFields, path: [] },
        { row: form.dropout_duration_ms, fields: dropoutFields, path: ['dropout_duration_ms'] },
    ];
}

function toForm(name, preset) {
    return {
        name,
        ...convertObject(presetFields, preset, fieldFromServer),
        dropout_duration_ms: [...preset.dropout_duration_ms],
    };
}

function toBody(form) {
    return {
        ...convertObject(presetFields, form, fieldToServer),
        dropout_duration_ms: form.dropout_duration_ms,
    };
}

function blankForm() {
    return { name: '', ...blankRow(presetFields), dropout_duration_ms: [0, 0] };
}

const presetEditor = registryEditor({
    path: '/api/signal-presets',
    prefix: 'preset',
    flagsDefaults: true,
    nameInput: 'preset-editor-name',
    list: 'presets',
    kind: 'signal-presets',
    label: 'signal preset',
    toForm,
    toBody,
    blankForm,
    blocks,
});

export function signalPresets() {
    return {
        ...presetEditor.state(),
        ...presetEditor.methods,
        ...signalPresetForms(),
        signalPresetSaving: false,
        presetFieldRows() {
            return blocks(this.presetForm).flatMap(({ row, fields, path }) =>
                fields.map((field) => ({ row, field, path: [...path, field.key] })),
            );
        },

        async changeSignalPreset() {
            const body = {
                tier: this.signalPresetSetting,
                preset: this.signalPresetName || null,
            };

            if (body.preset && this.signalPresetFade !== null && this.signalPresetFade !== '') {
                body.fade_duration = Number.parseFloat(this.signalPresetFade);
            }
            if (body.preset && this.signalPresetDuration !== null && this.signalPresetDuration !== '') {
                body.duration = Number.parseFloat(this.signalPresetDuration);
            }

            await this.runAction({
                saving: 'signalPresetSaving',
                label: 'changing signal preset',
                request: () =>
                    apiRequest(`/api/guilds/${this.route.name}/dj/signal-preset`, {
                        method: 'POST',
                        body,
                    }),
                success: 'Signal preset changed successfully!',
                failure: 'Failed to change signal preset',
            });
        },
    };
}
