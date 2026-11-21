import { apiRequest } from './api.js';
import { newItem, registryEditor } from './registry-editor.js';
import { blankForm, toBody, toForm } from './signal-chain.js';

export const signalPresetForms = () => ({
    signalPresetSetting: 'manual',
    signalPresetName: '',
    signalPresetFade: null,
    signalPresetDuration: null,
});

const kind = 'signal-presets';

const presetEditor = registryEditor({
    path: '/api/signal-presets',
    prefix: 'preset',
    flagsDefaults: true,
    nameInput: 'chain-new-name',
    list: 'presets',
    kind,
    label: 'signal preset',
    toForm,
    toBody,
    blankForm,
});

export function signalPresets() {
    return {
        ...presetEditor.state(),
        ...presetEditor.methods,
        ...signalPresetForms(),
        signalPresetSaving: false,

        presetChain() {
            const app = this;
            return {
                get active() {
                    return app.route.kind === kind;
                },
                get name() {
                    return app.route.name;
                },
                get draft() {
                    return app.route.kind === kind && !app.presetEditorLoading
                        ? app.drafts[kind][app.route.name]
                        : undefined;
                },
                get isNew() {
                    return app.route.name === newItem;
                },
                get saving() {
                    return app.presetEditorSaving;
                },
                get names() {
                    return app.presets;
                },
                isDefault: (name) => app.isDefault(kind, name),
                open: (name) => app.openItem(kind, name),
                save: () => app.applyDraft(kind),
                saveAs: (name) => {
                    app.saveAsName = name;
                    return app.saveAs(kind);
                },
            };
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
