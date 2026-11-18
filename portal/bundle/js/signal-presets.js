import { apiRequest } from './api.js';
import { blankRow, convertObject, fieldFromServer, fieldToServer } from './form-fields.js';
import { registryEditor } from './registry-editor.js';

export const signalPresetForms = () => ({
    signalPresetSetting: 'manual',
    signalPresetName: '',
    signalPresetFade: null,
    signalPresetDuration: null,
});

const presetFields = [
    { key: 'bandpass_low', label: 'Bandpass Low (Hz)', type: 'number', step: 'any', min: 0, default: 20 },
    { key: 'bandpass_high', label: 'Bandpass High (Hz)', type: 'number', step: 'any', min: 0, default: 20000 },
    { key: 'white_noise_level', label: 'White Noise Level (0-2)', type: 'number', step: 'any', min: 0, max: 2 },
    { key: 'pink_noise_level', label: 'Pink Noise Level (0-2)', type: 'number', step: 'any', min: 0, max: 2 },
    { key: 'brown_noise_level', label: 'Brown Noise Level (0-2)', type: 'number', step: 'any', min: 0, max: 2 },
    { key: 'tremolo_depth', label: 'Tremolo Depth', type: 'number', step: 'any' },
    { key: 'tremolo_rate', label: 'Tremolo Rate (Hz)', type: 'number', step: 'any' },
    { key: 'tremolo_jitter', label: 'Tremolo Jitter', type: 'number', step: 'any' },
    { key: 'clip_pregain', label: 'Clip Pregain', type: 'number', step: 'any', default: 1 },
    { key: 'clip_threshold', label: 'Clip Threshold', type: 'number', step: 'any', default: 1 },
    { key: 'bitcrush_bits', label: 'Bitcrush Bits', type: 'number', step: 1, min: 1, max: 255, optional: true },
    { key: 'dropout_probability', label: 'Dropout Probability', type: 'number', step: 'any', min: 0 },
];

const dropoutFields = [
    { key: 0, label: 'Dropout Min Duration (ms)', type: 'number', step: 'any', min: 0 },
    { key: 1, label: 'Dropout Max Duration (ms)', type: 'number', step: 'any', min: 0 },
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
