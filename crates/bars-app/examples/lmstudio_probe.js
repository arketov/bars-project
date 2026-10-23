// Программная проверка собранного WebView2; профиль и воркер изолированы.
(async () => {
  const invoke = window.__TAURI_INTERNALS__.invoke;
  const checks = [];
  const wait = (predicate, label) => new Promise((resolve, reject) => {
    const end = performance.now() + 15000;
    const poll = () => { const value = predicate(); if (value) resolve(value); else if (performance.now() > end) reject(Error(label)); else setTimeout(poll, 30); }; poll();
  });
  const button = label => [...document.querySelectorAll('button')].find(b => b.textContent.trim() === label);
  const assert = (value, label) => {if (!value) throw Error(label); checks.push(label);};
  const frames = () => new Promise(resolve => setTimeout(resolve, 50));
  try {
    (await wait(() => button('Панели ▾'), 'меню панелей')).click();
    (await wait(() => button('Настройки'), 'пункт настроек')).click();
    (await wait(() => button('LM Studio'), 'источник LM Studio')).click();
    await frames();
    assert([...document.querySelectorAll('button')].filter(b=>b.textContent.trim()==='LM Studio').length===1, 'LM Studio только в embed');
    const panel = button('LM Studio').closest('.app');
    assert(panel.textContent.includes('обрезать длинный вход'), 'предупреждение об усечении');
    const inputs = [...panel.querySelectorAll('input')];
    const address = inputs.find(i => i.value==='http://127.0.0.1:1234');
    assert(!!address, 'исходный адрес LM Studio');
    // Точный идентификатор из сервера пользователь вводит в поле модели.
    const modelInput = [...panel.querySelectorAll('.field')].find(f => f.querySelector('b')?.textContent==='Модель')?.querySelector('input');
    assert(!!modelInput, 'поле идентификатора');
    modelInput.value='text-embedding-qwen3-embedding-0.6b';
    modelInput.dispatchEvent(new Event('input',{bubbles:true}));
    await wait(()=>button('Сохранить')&&!button('Сохранить').disabled,'валидные настройки');
    button('Сохранить').click();
    await wait(()=>button('Сохранить')?.disabled&&document.body.textContent.includes('Настройки сохранены'),'сохранение');
    const saved = await invoke('bars_call',{method:'settings_get',params:{}});
    assert(saved.settings.embed.source==='lmstudio', 'настоящий IPC: источник сохранён');
    assert(saved.settings.embed.model==='text-embedding-qwen3-embedding-0.6b', 'настоящий IPC: модель сохранена');
    assert(saved.settings.rerank.source==='builtin', 'независимый rerank сохранён');
    for (const theme of ['light','dark']) for (const width of [1440,1024,760]) {
      document.documentElement.dataset.theme=theme;
      await invoke('lmstudio_probe_size',{width}); await frames();
      const source=button('LM Studio').getBoundingClientRect();
      assert(source.width>0&&source.right<=innerWidth,`${theme}/${width}: выбор LM Studio в окне`);
    }
    await invoke('lmstudio_probe_finish',{result:{success:true,checks}});
  } catch (error) {await invoke('lmstudio_probe_finish',{result:{success:false,checks,error:String(error),stack:error.stack}});}
})();
