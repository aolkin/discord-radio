import { apiRequest, bodyOrStatus, emptyMessage } from './api.js';

const capitalize = (text) => text.charAt(0).toUpperCase() + text.slice(1);

export function registryEditor({ path, prefix, list, label, toForm, toBody, blankForm }) {
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
            [`show${capitalize(editor)}`]: false,
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
                this.openModal(editor);

                if (this[isNew]) {
                    this[form] = blankForm();
                    return;
                }

                this[loading] = true;
                try {
                    const response = await apiRequest(`${path}/${encodeURIComponent(name)}`, {
                        errorMessage: bodyOrStatus,
                    });
                    this[form] = toForm(name, await response.json());
                } catch (error) {
                    console.error(`Failed to load ${label}:`, error);
                    this[message] = { text: `Failed to load ${label}: ${error.message}`, type: 'error' };
                } finally {
                    this[loading] = false;
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
                        this[isNew] = false;
                        await this[loadList]();
                    },
                    success: `${capitalize(label)} saved`,
                    failure: `Failed to save ${label}`,
                });
            },
        },
    };
}
