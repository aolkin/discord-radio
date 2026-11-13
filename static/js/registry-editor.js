import { apiRequest, bodyOrStatus, emptyMessage } from './api.js';
import { fieldInvalid } from './form-fields.js';

export const newItem = '+new';

const capitalize = (text) => text.charAt(0).toUpperCase() + text.slice(1);

const specs = {};

const clone = (value) => JSON.parse(JSON.stringify(value));

const isContainer = (value) => value !== null && typeof value === 'object';

function diff(saved, form, path = []) {
    if (isContainer(saved) && isContainer(form)) {
        const keys = new Set([...Object.keys(saved), ...Object.keys(form)]);
        return [...keys].flatMap((key) =>
            key in saved && key in form
                ? diff(saved[key], form[key], [...path, key])
                : [{ path: [...path, key], saved: saved[key], form: form[key] }],
        );
    }
    return saved === form ? [] : [{ path, saved, form }];
}

const getPath = (value, path) => path.reduce((node, key) => (isContainer(node) ? node[key] : undefined), value);

function formatValue(value) {
    if (value === undefined || value === null || value === '') {
        return '(not set)';
    }
    return isContainer(value) ? JSON.stringify(value) : String(value);
}

const pathLabel = (path) =>
    path.map((key) => (/^\d+$/.test(key) ? `#${Number(key) + 1}` : String(key).replaceAll('_', ' '))).join(' › ');

export function registryEditor({
    path,
    prefix,
    list,
    kind,
    label,
    toForm,
    toBody,
    blankForm,
    blocks,
    formErrors,
    afterSave,
}) {
    const editor = `${prefix}Editor`;
    const form = `${prefix}Form`;
    const isNew = `${editor}IsNew`;
    const loading = `${editor}Loading`;
    const saving = `${editor}Saving`;
    const message = `${editor}Message`;
    const loadList = `load${capitalize(list)}`;

    specs[kind] = {
        path,
        list,
        label,
        toBody,
        blocks,
        formErrors,
        afterSave,
        form,
        isNew,
        loading,
        saving,
        message,
        loadList,
    };

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
                const key = name ?? newItem;
                this[isNew] = name === null;
                this[message] = emptyMessage();
                this.draftReviewing = false;
                this.saveAsName = null;
                this[loading] = false;

                const draft = this.drafts[kind][key];
                if (draft && (this[isNew] || this.changeCount(kind, key) > 0)) {
                    this[form] = draft.form;
                    return;
                }
                if (this[isNew]) {
                    this.setDraft(kind, key, blankForm());
                    return;
                }

                this[form] = blankForm();
                this[loading] = true;
                try {
                    const response = await apiRequest(`${path}/${encodeURIComponent(name)}`, {
                        errorMessage: bodyOrStatus,
                    });
                    const data = await response.json();
                    if (this.isSelected(kind, name)) {
                        this.setDraft(kind, key, toForm(name, data));
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
        },
    };
}

export function drafts() {
    return {
        drafts: Object.fromEntries(Object.keys(specs).map((kind) => [kind, {}])),
        draftReviewing: false,
        saveAsName: null,
        fieldInvalid,

        setDraft(kind, key, form) {
            this.drafts[kind][key] = { saved: clone(form), form };
            this[specs[kind].form] = this.drafts[kind][key].form;
        },

        openDraft(kind) {
            return this.route.kind === kind ? this.drafts[kind][this.route.name] : undefined;
        },

        editorStatus(kind) {
            const spec = specs[kind];
            return {
                message: this[spec.message],
                saving: this[spec.saving],
                loading: this[spec.loading],
                isNew: this[spec.isNew],
            };
        },

        changeCount(kind, key) {
            const draft = this.drafts[kind][key];
            return draft ? diff(draft.saved, draft.form).length : 0;
        },

        isChanged(kind, path) {
            const draft = this.openDraft(kind);
            if (!draft) {
                return false;
            }
            const parent = getPath(draft.saved, path.slice(0, -1));
            return (
                isContainer(parent) &&
                path.at(-1) in parent &&
                diff(getPath(draft.saved, path), getPath(draft.form, path)).length > 0
            );
        },

        wasValue(kind, path) {
            return formatValue(getPath(this.openDraft(kind)?.saved, path));
        },

        reviewList(kind) {
            const draft = this.openDraft(kind);
            return draft
                ? diff(draft.saved, draft.form).map((change) => ({
                      label: pathLabel(change.path),
                      was: formatValue(change.saved),
                      now: formatValue(change.form),
                  }))
                : [];
        },

        nameInvalid(kind) {
            const spec = specs[kind];
            const name = this[spec.form].name;
            return this[spec.isNew] && (!/^[\w-]+$/.test(name) || this[spec.list].includes(name));
        },

        draftErrors(kind) {
            const draft = this.openDraft(kind);
            if (!draft) {
                return [];
            }
            const spec = specs[kind];
            const errors = [...(spec.formErrors?.(draft.form) ?? [])];
            if (this.nameInvalid(kind)) {
                errors.unshift('Name must be new and use only letters, digits, - and _');
            }
            const fieldsInvalid = spec
                .blocks(draft.form)
                .some(({ row, fields }) => fields.some((field) => fieldInvalid(field, row[field.key])));
            if (fieldsInvalid) {
                errors.push('Fix the marked fields');
            }
            return errors;
        },

        reviewDraft(kind) {
            if (this.changeCount(kind, this.route.name) > 0 && this.draftErrors(kind).length === 0) {
                this.draftReviewing = true;
            }
        },

        discardDraft(kind) {
            const draft = this.openDraft(kind);
            draft.form = clone(draft.saved);
            this[specs[kind].form] = draft.form;
            this[specs[kind].message] = emptyMessage();
            this.draftReviewing = false;
        },

        openSaveAs(kind) {
            const taken = this[specs[kind].list];
            const base = `${this.route.name}_copy`;
            let name = base;
            for (let count = 2; taken.includes(name); count++) {
                name = `${base}${count}`;
            }
            this.saveAsName = name;
        },

        saveAsInvalid(kind) {
            return !/^[\w-]+$/.test(this.saveAsName) || this[specs[kind].list].includes(this.saveAsName);
        },

        async saveAs(kind) {
            const spec = specs[kind];
            const draft = this.drafts[kind][this.route.name];
            const name = this.saveAsName;

            await this.runAction({
                saving: spec.saving,
                message: spec.message,
                label: `saving ${spec.label} as ${name}`,
                request: () =>
                    apiRequest(`${spec.path}/${encodeURIComponent(name)}`, {
                        method: 'POST',
                        body: spec.toBody(draft.form),
                        errorMessage: bodyOrStatus,
                    }),
                onSuccess: async () => {
                    draft.form = clone(draft.saved);
                    this.saveAsName = null;
                    this.draftReviewing = false;
                    await this[spec.loadList]();
                    await spec.afterSave?.call(this);
                    this.openItem(kind, name);
                },
                success: `${capitalize(spec.label)} saved as ${name}`,
                failure: `Failed to save ${spec.label} as ${name}`,
            });
        },

        async applyDraft(kind) {
            const spec = specs[kind];
            const key = this.route.name;
            const draft = this.drafts[kind][key];
            const name = draft.form.name;

            await this.runAction({
                saving: spec.saving,
                message: spec.message,
                label: `saving ${spec.label}`,
                request: () =>
                    apiRequest(`${spec.path}/${encodeURIComponent(name)}`, {
                        method: 'PUT',
                        body: spec.toBody(draft.form),
                        errorMessage: bodyOrStatus,
                    }),
                onSuccess: async () => {
                    draft.saved = clone(draft.form);
                    this.draftReviewing = false;
                    if (key === newItem) {
                        delete this.drafts[kind][newItem];
                        this.drafts[kind][name] = draft;
                        if (this.isSelected(kind, newItem)) {
                            this[spec.isNew] = false;
                            this.replaceRoute({ view: 'edit', kind, name });
                        }
                    }
                    await this[spec.loadList]();
                    await spec.afterSave?.call(this);
                },
                success: `${capitalize(spec.label)} saved`,
                failure: `Failed to save ${spec.label}`,
            });
        },
    };
}
