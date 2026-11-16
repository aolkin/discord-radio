document.addEventListener('alpine:init', () => {
    Alpine.directive('audio-stream', (element, _directive, { cleanup }) => {
        cleanup(() => {
            element.pause();
            element.removeAttribute('src');
            element.load();
        });
    });
});
