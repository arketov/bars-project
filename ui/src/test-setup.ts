// jsdom не выполняет раскладку. Геометрия и жесты проверяются в WebView2/браузере.
globalThis.ResizeObserver ??= class {
  observe() {} unobserve() {} disconnect() {}
};
Element.prototype.scrollIntoView ??= function () {};
