// jsdom не выполняет раскладку. Геометрия и жесты проверяются в WebView2/браузере.
globalThis.ResizeObserver ??= class {
  observe() {} unobserve() {} disconnect() {}
};
Element.prototype.scrollIntoView ??= function () {};
// CodeMirror измеряет текст через Range; в jsdom геометрии нет, отдаём пустые прямоугольники.
const emptyRect = { x: 0, y: 0, top: 0, left: 0, right: 0, bottom: 0, width: 0, height: 0, toJSON() { return {}; } } as DOMRect;
Range.prototype.getClientRects ??= () => [] as unknown as DOMRectList;
Range.prototype.getBoundingClientRect ??= () => emptyRect;
