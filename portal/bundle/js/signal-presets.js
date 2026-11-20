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

const noiseFields = [
    limited('white', 'White Noise Level (0-2)', 0, 2, { slider: linear(2) }),
    limited('pink', 'Pink Noise Level (0-2)', 0, 2, { slider: linear(2) }),
    limited('brown', 'Brown Noise Level (0-2)', 0, 2, { slider: linear(2) }),
];

const stageFields = {
    bandpass: [
        limited('low', 'Bandpass Low (Hz)', 20, 20000, { default: 20, slider: log(20, 20000, 1) }),
        limited('high', 'Bandpass High (Hz)', 20, 20000, { default: 20000, slider: log(20, 20000, 1) }),
    ],
    tremolo: [
        limited('depth', 'Tremolo Depth', 0, 1, { slider: linear(1) }),
        limited('rate', 'Tremolo Rate (Hz)', 0, 20, { slider: log(0.1, 20, 0.01) }),
        limited('jitter', 'Tremolo Jitter', 0, 1, { slider: linear(1) }),
    ],
    clip: [
        limited('pregain', 'Clip Pregain', 0, 100, { default: 1 }),
        limited('threshold', 'Clip Threshold', 0, 2, { default: 1, slider: linear(2) }),
    ],
    bitcrush: [{ key: 'bits', label: 'Bitcrush Bits', type: 'number', step: 1, min: 1, max: 16, optional: true }],
    dropout: [
        limited('probability', 'Dropout Probability', 0, 1, { slider: linear(1) }),
        limited('min_ms', 'Dropout Min Duration (ms)', 0, 2000, { slider: log(1, 2000, 1) }),
        limited('max_ms', 'Dropout Max Duration (ms)', 0, 2000, { slider: log(1, 2000, 1) }),
    ],
};

const mapStages = (convert) =>
    Object.fromEntries(Object.entries(stageFields).map(([stage, fields]) => [stage, convert(stage, fields)]));

function blocks(form) {
    return [
        { row: form.noise, fields: noiseFields, path: ['noise'] },
        ...Object.entries(stageFields).map(([stage, fields]) => ({ row: form[stage], fields, path: [stage] })),
    ];
}

function toForm(name, preset) {
    return {
        name,
        noise: {
            ...blankRow(noiseFields),
            ...Object.fromEntries(preset.noise.map(({ kind, level }) => [kind, level])),
        },
        ...mapStages((stage, fields) => convertObject(fields, preset[stage], fieldFromServer)),
    };
}

function toBody(form) {
    return {
        noise: noiseFields
            .filter(({ key }) => form.noise[key] > 0)
            .map(({ key }) => ({ kind: key, level: form.noise[key] })),
        ...mapStages((stage, fields) => convertObject(fields, form[stage], fieldToServer)),
    };
}

function blankForm() {
    return { name: '', noise: blankRow(noiseFields), ...mapStages((_, fields) => blankRow(fields)) };
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
