import { apiRequest, bodyOrStatus, emptyMessage } from './api.js';

const capitalize = (text) => text.charAt(0).toUpperCase() + text.slice(1);

export function registryEditor({ path, prefix, list, kind, label, toForm, toBody, blankForm, afterSave }) {
    const editor = `${prefix}Editor`;
    const form = `${prefix}Form`;
    const isNew = `${editor}IsNew`;
    const loading = `${editor}Loading`;
    const saving = `${editor}Saving`;
    const message = `${editor}Message`;
    const loadList = `load${capitalize(list)}`;

    return {
        state: () => ({
            [list]: [],
            [isNew]: false,
            [loading]: false,
            [saving]: false,
            [message]: emptyMessage(),
            [form]: blankForm(),
        }),

        methods: {
            async [loadList]() {
                try {
                    const response = await apiRequest(path);
                    const data = await response.json();
                    this[list] = data.sort();
                } catch (error) {
                    console.error(`Failed to load ${label}s:`, error);
                }
            },

            async [`open${capitalize(editor)}`](name) {
                this[isNew] = name === null;
                this[message] = emptyMessage();
                this[form] = blankForm();
                this[loading] = !this[isNew];

                if (this[isNew]) {
                    return;
                }

                try {
                    const response = await apiRequest(`${path}/${encodeURIComponent(name)}`, {
                        errorMessage: bodyOrStatus,
                    });
                    const data = await response.json();
                    if (this.isSelected(kind, name)) {
                        this[form] = toForm(name, data);
                    }
                } catch (error) {
                    console.error(`Failed to load ${label}:`, error);
                    if (this.isSelected(kind, name)) {
                        this[message] = { text: `Failed to load ${label}: ${error.message}`, type: 'error' };
                    }
                } finally {
                    if (this.isSelected(kind, name)) {
                        this[loading] = false;
                    }
                }
            },

            async [`save${capitalize(prefix)}`]() {
                const name = this[form].name;
                if (this[isNew] && this[list].includes(name)) {
                    this[message] = { text: `A ${label} named "${name}" already exists`, type: 'error' };
                    return;
                }

                await this.runAction({
                    saving,
                    message,
                    label: `saving ${label}`,
                    request: () =>
                        apiRequest(`${path}/${encodeURIComponent(name)}`, {
                            method: 'PUT',
                            body: toBody(this[form]),
                            errorMessage: bodyOrStatus,
                        }),
                    onSuccess: async () => {
                        if (this[isNew]) {
                            this[isNew] = false;
                            this.replaceRoute({ view: 'edit', kind, name });
                        }
                        await this[loadList]();
                        await afterSave?.call(this);
                    },
                    success: `${capitalize(label)} saved`,
                    failure: `Failed to save ${label}`,
                });
            },
        },
    };
}
