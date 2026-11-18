document.addEventListener('alpine:init', () => {
    Alpine.directive('scroll-fade', (element, _directive, { cleanup }) => {
        const update = () => {
            const end = element.scrollWidth - element.clientWidth;
            element.classList.toggle('fade-start', element.scrollLeft > 1);
            element.classList.toggle('fade-end', end - element.scrollLeft > 1);
        };
        const observer = new ResizeObserver(update);
        observer.observe(element);
        if (element.firstElementChild) {
            observer.observe(element.firstElementChild);
        }
        element.addEventListener('scroll', update, { passive: true });
        cleanup(() => {
            observer.disconnect();
            element.removeEventListener('scroll', update);
        });
    });

    Alpine.directive('stuck', (element, _directive, { cleanup }) => {
        const observer = new IntersectionObserver(
            ([entry]) => {
                element.classList.toggle('is-stuck', entry.intersectionRatio < 1 && entry.boundingClientRect.top < 0);
            },
            { threshold: [1] },
        );
        observer.observe(element);
        cleanup(() => observer.disconnect());
    });
});

export function editTree() {
    return {
        treeOpen: false,

        openTree() {
            this.treeOpen = true;
            this.$nextTick(() => {
                const tree = this.$refs.editTree;
                (tree.querySelector('.tree-item.btn-primary') ?? tree.querySelector('button')).focus();
            });
        },

        closeTree() {
            if (!this.treeOpen) {
                return;
            }
            this.treeOpen = false;
            this.$refs.editTreeToggle.focus();
        },

        treeClick(event) {
            if (event.target.closest('button')) {
                this.closeTree();
            }
        },

        treeKeydown(event) {
            if (!this.treeOpen || event.key !== 'Tab') {
                return;
            }
            const buttons = [...this.$refs.editTree.querySelectorAll('button')].filter(
                (button) => button.offsetParent !== null,
            );
            const first = buttons[0];
            const last = buttons[buttons.length - 1];
            if (event.shiftKey && document.activeElement === first) {
                event.preventDefault();
                last.focus();
            } else if (!event.shiftKey && document.activeElement === last) {
                event.preventDefault();
                first.focus();
            }
        },
    };
}
