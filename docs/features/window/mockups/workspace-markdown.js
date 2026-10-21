/* Markdown макета: та же установленная marked 18.1.1, что в UI; без сырого HTML и загрузки изображений. */
(() => {
  const escape = value => String(value).replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
  window.renderMockDocument = (text, hit, linkTarget) => {
    const parser = new marked.Marked({ gfm: true, breaks: false });
    parser.use({ renderer: {
      html(token) { return escape(token.text); },
      image(token) { return `<span class="a">${escape(token.text || token.href)}</span>`; },
      link(token) {
        const target = linkTarget(token.href);
        const label = this.parser.parseInline(token.tokens);
        return target ? `<button class="render-link" data-open="${escape(target.path)}" data-target-right="${target.side ? 'true' : 'false'}">${label}</button>` : `<span class="a">${label}</span>`;
      },
    }});
    const frontmatter = /^---\r?\n[\s\S]*?\r?\n---[ \t]*(\r?\n|$)/.exec(text);
    const skipped = frontmatter ? frontmatter[0].split('\n').length - 1 : 0;
    const body = frontmatter ? text.slice(frontmatter[0].length) : text;
    let line = skipped + 1, labelled = false;
    const metadataHit = hit && hit.first <= skipped
      ? `<section class="preview-block hit metadata-match" data-first="${hit.first}" data-last="${Math.min(hit.last, skipped)}"><span class="hitlabel">строки ${hit.first}–${Math.min(hit.last, skipped)} · найдено в свойствах документа</span><pre>${escape(text.split('\n').slice(hit.first - 1, Math.min(hit.last, skipped)).join('\n'))}</pre></section>`
      : '';
    return metadataHit + parser.lexer(body).map(token => {
      const first = line;
      const count = (token.raw.match(/\n/g) || []).length;
      const last = Math.max(first, first + count - (token.raw.endsWith('\n') ? 1 : 0));
      line += count;
      if (token.type === 'space') return '';
      const found = hit && first <= hit.last && last >= hit.first;
      const label = found && !labelled ? `<span class="hitlabel">строки ${hit.first}–${hit.last} · найдено</span>` : '';
      if (found) labelled = true;
      return `<section class="preview-block${found ? ' hit' : ''}" data-first="${first}" data-last="${last}">${label}${parser.parser([token])}</section>`;
    }).join('\n');
  };
})();
