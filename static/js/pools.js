import { apiRequest, bodyOrStatus } from './api.js';
import {
    blankRow,
    blankSegmentDefaults,
    convertObject,
    entryPlaybackFields,
    fieldFromServer,
    fieldToServer,
    noisePeriodFields,
    optionalSeconds,
    segmentDefaultsBlocks,
    segmentDefaultsFromServer,
    segmentDefaultsToServer,
    toggleHexConfig,
    weight,
} from './form-fields.js';
import { registryEditor } from './registry-editor.js';

const kinds = {
    track: {
        label: 'Track',
        identifier: 'filename',
        what: ['filename'],
        length: ['max_duration_seconds'],
        expansion: ['allow_subsection', 'volume', 'channel_status', 'announcement'],
        entryFields: [
            { key: 'filename', label: 'Filename', type: 'text' },
            weight,
            { ...optionalSeconds('max_duration_seconds', 'Max Duration (s)'), placeholder: 'Whole file' },
            { key: 'allow_subsection', label: 'Allow Subsection', type: 'bool', optional: true },
            { key: 'volume', label: 'Volume (0-2)', type: 'number', step: 'any', min: 0, max: 2, optional: true },
            ...entryPlaybackFields,
        ],
    },
    hex: {
        label: 'Hex Message',
        identifier: 'text',
        what: ['text'],
        length: ['loop_min', 'loop_max'],
        expansion: ['channel_status', 'announcement'],
        entryFields: [
            { key: 'text', label: 'Text', type: 'text' },
            weight,
            { key: 'loop_min', label: 'Loop Min', type: 'number', step: 1, min: 0, optional: true },
            { key: 'loop_max', label: 'Loop Max', type: 'number', step: 1, min: 0, optional: true },
            ...entryPlaybackFields,
        ],
    },
    noise: {
        label: 'Noise',
        identifier: 'min_duration_seconds',
        what: [],
        length: ['min_duration_seconds', 'max_duration_seconds'],
        expansion: ['fade_duration_seconds', 'channel_status', 'announcement'],
        entryFields: [...noisePeriodFields, ...entryPlaybackFields],
    },
};

const sameRow = (a, b) => Alpine.raw(a) === Alpine.raw(b);

const isIn = (rows, entry) => rows.some((row) => sameRow(row, entry));

export function poolToForm(name, pool) {
    return {
        name,
        segment_defaults: segmentDefaultsFromServer(pool.segment_defaults),
        entries: pool.entries.map((entry) => {
            const kind = Object.keys(kinds).find((kindName) => kinds[kindName].identifier in entry);
            return { kind, ...convertObject(kinds[kind].entryFields, entry, fieldFromServer) };
        }),
    };
}

function toBody(form) {
    return {
        segment_defaults: segmentDefaultsToServer(form.segment_defaults),
        entries: form.entries.map((entry) => convertObject(kinds[entry.kind].entryFields, entry, fieldToServer)),
    };
}

function blankForm() {
    return {
        name: '',
        segment_defaults: blankSegmentDefaults(),
        entries: [],
    };
}

function blocks(form) {
    return [
        ...segmentDefaultsBlocks(form.segment_defaults, ['segment_defaults']),
        ...form.entries.map((entry, index) => ({
            row: entry,
            fields: kinds[entry.kind].entryFields,
            label: `${kinds[entry.kind].label} Entry`,
            path: ['entries', index],
            index,
        })),
    ];
}

function formErrors(form) {
    if (form.entries.length === 0) {
        return ['Add at least one entry'];
    }
    return form.entries.some((entry) => typeof entry.weight === 'number' && entry.weight > 0)
        ? []
        : ['Give at least one entry a weight above 0'];
}

const poolEditor = registryEditor({
    path: '/api/pools',
    prefix: 'pool',
    nameInput: 'pool-editor-name',
    list: 'pools',
    kind: 'pools',
    label: 'pool',
    toForm: poolToForm,
    toBody,
    blankForm,
    blocks,
    formErrors,
    afterSave(created) {
        if (created) {
            this.addCreatedPoolToDjConfig(created);
        }
    },
});

const hexFolderDebounceMs = 400;

const usesHex = (form) =>
    form.segment_defaults.hex_config !== null || form.entries.some((entry) => entry.kind === 'hex');

function hexFolderWarning({ folder, missing_files: missingFiles }) {
    return missingFiles.length > 0 ? `Hex audio folder "${folder}" is missing ${missingFiles.join(', ')}.` : '';
}

export function pools() {
    let hexFolderKey = null;
    let hexFolderName = null;
    let hexFolderTimer;

    return {
        ...poolEditor.state(),
        ...poolEditor.methods,
        poolHexFolderWarning: '',
        poolMode: 'properties',
        poolExpanded: {},
        poolSelected: [],
        poolKinds: kinds,
        poolWeightField: weight,

        poolUsers(pool) {
            return this.djConfigs.filter((config) => this.djConfigPools[config]?.includes(pool));
        },

        togglePoolHexConfig() {
            toggleHexConfig(this.poolForm.segment_defaults);
        },

        watchPoolHexFolder() {
            const active = this.route.view === 'edit' && this.route.kind === 'pools' && usesHex(this.poolForm);
            const folder = active ? (this.poolForm.segment_defaults.hex_config?.hex_audio_folder ?? '').trim() : '';
            const key = active ? `${this.route.name}\n${folder}` : null;
            if (key === hexFolderKey) {
                return;
            }
            const opened = hexFolderName !== this.route.name;
            hexFolderKey = key;
            hexFolderName = this.route.name;
            clearTimeout(hexFolderTimer);
            if (key === null) {
                this.poolHexFolderWarning = '';
                return;
            }
            hexFolderTimer = setTimeout(
                async () => {
                    let warning;
                    try {
                        const query = folder === '' ? '' : `?folder=${encodeURIComponent(folder)}`;
                        const response = await apiRequest(`/api/hex-audio-folder${query}`, {
                            errorMessage: bodyOrStatus,
                        });
                        warning = hexFolderWarning(await response.json());
                    } catch (error) {
                        warning = `Could not check the hex audio folder: ${error.message}`;
                    }
                    if (key === hexFolderKey) {
                        this.poolHexFolderWarning = warning;
                    }
                },
                opened ? 0 : hexFolderDebounceMs,
            );
        },

        poolDefaultBlocks() {
            return segmentDefaultsBlocks(this.poolForm.segment_defaults, ['segment_defaults']);
        },

        poolColumn(entry, column) {
            const { entryFields } = kinds[entry.kind];
            return kinds[entry.kind][column].map((key) => entryFields.find((field) => field.key === key));
        },

        poolProfileOptions(value) {
            return value === '' || this.profiles.includes(value) ? this.profiles : [...this.profiles, value];
        },

        poolEntryName(entry) {
            if (entry.kind === 'noise') {
                return `Noise ${entry.min_duration_seconds}-${entry.max_duration_seconds}s`;
            }
            return entry[kinds[entry.kind].identifier];
        },

        pickPoolFile(entry) {
            this.openFileBrowser({
                mode: 'file',
                prefix: 'tracks/',
                onSelect: (key) => {
                    entry.filename = `s3://${key}`;
                },
            });
        },

        pickPoolFiles() {
            this.openFileBrowser({
                mode: 'files',
                prefix: 'tracks/',
                onSelect: (keys) => {
                    for (const key of keys) {
                        this.poolForm.entries.push({
                            kind: 'track',
                            ...blankRow(kinds.track.entryFields),
                            filename: `s3://${key}`,
                        });
                    }
                },
            });
        },

        addPoolEntry(kind) {
            this.poolForm.entries.push({ kind, ...blankRow(kinds[kind].entryFields) });
            this.focusWhenShown('.pool-grid-entries tbody:last-of-type .pool-grid-cell input');
        },

        movePoolEntry(index, offset) {
            const target = index + offset;
            if (target < 0 || target >= this.poolForm.entries.length) {
                return;
            }
            const [entry] = this.poolForm.entries.splice(index, 1);
            this.poolForm.entries.splice(target, 0, entry);
            this.poolExpanded = {};
        },

        removePoolEntry(index) {
            this.poolForm.entries.splice(index, 1);
            this.poolExpanded = {};
        },

        poolSelectedRows() {
            return this.poolForm.entries.filter((entry) => isIn(this.poolSelected, entry));
        },

        poolAllSelected() {
            return this.poolForm.entries.length > 0 && this.poolSelectedRows().length === this.poolForm.entries.length;
        },

        togglePoolSelectAll() {
            this.poolSelected = this.poolAllSelected() ? [] : [...this.poolForm.entries];
        },

        togglePoolSelected(entry) {
            this.poolSelected = isIn(this.poolSelected, entry)
                ? this.poolSelected.filter((row) => !sameRow(row, entry))
                : [...this.poolSelected, entry];
        },

        poolBulkDelete() {
            const rows = this.poolSelectedRows();
            if (!confirm(`Delete ${rows.length} selected ${rows.length === 1 ? 'entry' : 'entries'}?`)) {
                return;
            }
            this.poolForm.entries = this.poolForm.entries.filter((entry) => !isIn(rows, entry));
            this.poolSelected = [];
            this.poolExpanded = {};
        },

        poolBulkDuplicate() {
            const rows = this.poolSelectedRows();
            this.poolForm.entries = this.poolForm.entries.flatMap((entry) =>
                isIn(rows, entry) ? [entry, { ...Alpine.raw(entry) }] : [entry],
            );
            this.poolExpanded = {};
        },

        poolBulkEqualize() {
            const rows = this.poolSelectedRows();
            const total = rows.reduce((sum, row) => sum + (typeof row.weight === 'number' ? row.weight : 0), 0);
            const equal = Math.max(1, Math.round(total / rows.length));
            for (const row of rows) {
                row.weight = equal;
            }
        },
    };
}
