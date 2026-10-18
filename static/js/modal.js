export const modals = {};

// Sets a visibility property, which may be nested ('fileBrowser.open').
function setFlag(state, flag, value) {
    const path = flag.split('.');
    const last = path.pop();
    path.reduce((object, key) => object[key], state)[last] = value;
}

// `<template x-modal data-modal="name" ...>` renders the shared modal
// shell (overlay, header, close button, message area, and for modals
// with data-submit a form with a Cancel/Submit row) and puts the
// template's content in the body, or in the form when there is one.
// Open state comes from `modals[name]`; the other settings are
// data attributes:
//   data-overlay-class extra class on the overlay
//   data-title         header text
//   data-title-expression  expression for the header text, instead
//                      of data-title
//   data-message       property holding the { text, type } message;
//                      no message area when omitted
//   data-submit        expression run on form submit
//   data-saving        property that is true while submitting
//   data-disabled-when expression; Submit is also disabled while true
//   data-submit-label  Submit button text
//   data-busy-label    Submit button text while saving
//   data-cancel-label  Cancel button text (default "Cancel")
//   data-loading       property; while true, shows "Loading..." and
//                      hides the form
//   data-escape-unless expression; Escape does not close while true
document.addEventListener('alpine:init', () => {
    Alpine.directive('modal', (template) => {
        const config = template.dataset;
        const { flag } = modals[config.modal];
        const close = `closeModal(${JSON.stringify(config.modal)})`;

        const element = (tag, attributes = {}, className = '') => {
            const node = document.createElement(tag);
            if (className) node.className = className;
            for (const [name, value] of Object.entries(attributes)) {
                node.setAttribute(name, value);
            }
            return node;
        };

        const escapeHandler = config.escapeUnless ? `!(${config.escapeUnless}) && ${close}` : close;
        const overlay = element(
            'div',
            {
                'x-show': flag,
                'x-transition.opacity': '',
                'x-on:click.self': close,
                'x-on:keydown.escape.window': escapeHandler,
            },
            `modal-overlay ${config.overlayClass || ''}`.trim(),
        );
        const content = element('div', { 'x-show': flag, 'x-transition': '' }, 'modal-content');
        overlay.append(content);

        const header = element('div', {}, 'modal-header');
        const title = element('div', {}, 'modal-title');
        if (config.titleExpression) {
            title.setAttribute('x-text', config.titleExpression);
        } else {
            title.textContent = config.title;
        }
        const closeButton = element('button', { 'x-on:click': close }, 'close');
        closeButton.innerHTML = '&times;';
        header.append(title, closeButton);

        const body = element('div', {}, 'modal-body');
        if (config.message) {
            body.append(
                element('div', {
                    'x-show': `${config.message}.text`,
                    ':class': `${config.message}.type === 'error' ? 'error-message' : 'success-message'`,
                    'x-text': `${config.message}.text`,
                }),
            );
        }
        content.append(header, body);

        if (config.loading) {
            const loading = element('div', { 'x-show': config.loading }, 'loading');
            loading.textContent = 'Loading...';
            body.append(loading);
        }
        if (config.submit) {
            const form = element('form', { 'x-on:submit.prevent': config.submit });
            if (config.loading) form.setAttribute('x-show', `!${config.loading}`);
            const cancel = element('button', { type: 'button', 'x-on:click': close }, 'btn btn-secondary');
            cancel.textContent = config.cancelLabel || 'Cancel';
            const disabled = config.disabledWhen ? `${config.saving} || (${config.disabledWhen})` : config.saving;
            const submit = element('button', { type: 'submit', ':disabled': disabled }, 'btn btn-primary');
            submit.append(
                element('span', {
                    'x-text': `${config.saving} ? ${JSON.stringify(config.busyLabel)} : ${JSON.stringify(config.submitLabel)}`,
                }),
            );
            const buttons = element('div', {}, 'button-group');
            buttons.append(cancel, submit);
            form.append(template.content.cloneNode(true), buttons);
            body.append(form);
        } else {
            body.append(template.content.cloneNode(true));
        }

        template.after(overlay);
        Alpine.initTree(overlay);
    });
});

export const modalMethods = {
    // Modals named in `modals` (see above) open and close here.
    // Both reset the modal's fields.
    openModal(name, guildId) {
        const modal = modals[name];
        if (modal.perGuild) this.currentGuildId = guildId;
        Object.assign(this, modal.reset());
        setFlag(this, modal.flag, true);
    },

    closeModal(name) {
        const modal = modals[name];
        setFlag(this, modal.flag, false);
        if (modal.perGuild) this.currentGuildId = null;
        Object.assign(this, modal.reset());
    },
};
