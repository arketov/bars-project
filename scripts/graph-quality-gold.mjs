// Human-authored expectations, validated against frozen primary text before retrieval.
import fs from 'node:fs';
import crypto from 'node:crypto';
const [root, output] = process.argv.slice(2);
if (fs.existsSync(output)) throw Error('gold already frozen');
const cases = [];
const names = {
  local:'features/models/local-models.md', updates:'features/indexing/updates.md',
  chunk:'features/indexing/chunking.md', consent:'features/window/reindex-consent.md',
  search:'features/search/hybrid-search.md', gguf:'adr/0002-qwen-gguf.md',
  external:'features/models/external-models.md', clients:'features/clients/clients.md',
  projects:'features/projects/projects.md', deletion:'features/projects/deletion.md',
  secrets:'architecture/secrets.md', layout:'adr/0001-docs-by-features.md',
  topics:'features/new-topics/requirements.md',
};
function add(split, scenario, kind, query, request, refs, extras={}) {
  if (['context','relations'].includes(request.operation)) request={...request,query};
  const evidence=refs.map(([key,needle,role])=>{
    const path=names[key], bytes=fs.readFileSync(`${root}/docs/${path}`), text=bytes.toString('utf8');
    const at=text.indexOf(needle);
    if(at<0) throw Error(`gold quote not found: ${path}: ${needle}`);
    const start=Buffer.byteLength(text.slice(0,at));
    return {path,needle,start,end:start+Buffer.byteLength(needle),role,
      first_line:text.slice(0,at).split('\n').length,
      sha256:crypto.createHash('sha256').update(bytes).digest('hex')};
  });
  cases.push({id:`${split}-${scenario}-${kind}`,split,scenario,kind,query,
    request:{...request,budgets:{max_bytes:262144},limit:50},evidence,...extras});
}
const rel=query=>({operation:'relations',query});
const ctx=query=>({operation:'context',query});
const path=(from,to)=>({operation:'paths',from:{label:from,kind:'entity'},to:{label:to,kind:'entity'},path_kind:'semantic'});
const explore=(topic,analysis='topics')=>({operation:'explore',topic,analysis});

add('dev','S1','positive','Какие описания затронет изменение нарезки чанков?',{operation:'impact'},[
 ['updates','Если параметры нарезки или `docs_root`','behavior'],['chunk','Префикс документа модели','behavior']],
 {root_paths:[names.chunk],expectation:'Документированная зависимость индексатора и embeddings от нарезки; ссылки сами по себе не зависимости.'});
add('dev','S1','negative','Меняет ли нарезка чанков модель rerank?',{operation:'impact'},[
 ['updates','Согласие пользователя не нужно: модель не менялась','constraint']],
 {root_paths:[names.chunk],forbidden:'Нельзя выводить смену rerank из ссылки на модель.'});
add('holdout','S1','positive','Что затрагивает полное удаление проекта?',{operation:'impact'},[
 ['deletion','БД «Новых тем» `new-topics.sqlite3`','behavior'],['topics','A13','verification']],
 {root_paths:[names.deletion],expectation:'Удаление данных проекта затрагивает хранилище новых тем и приёмку A13.'});
add('holdout','S1','conditional','Удалит ли снятие регистрации документы проекта?',{operation:'impact'},[
 ['projects','это файлы пользователя','constraint'],['deletion','Markdown-документация и `BARS_INSTRUCTION.md` сохраняются побайтно','constraint']],
 {root_paths:[names.projects],forbidden:'unregister не получает разрушительную семантику project_delete.'});

add('dev','S2','positive','Как bge-m3 связано с fastembed и ORT?',path('bge-m3','fastembed'),[
 ['gguf','bge-m3 остаётся умолчанием на fastembed/ORT','behavior']],
 {facts:[{subject:'bge-m3',predicate:'uses',object:'fastembed/ORT',polarity:'positive'}]});
add('dev','S2','negative','Есть ли у Nomic подтверждённая зависимость от Vulkan?',path('Nomic','Vulkan'),[
 ['local','только CPU','constraint']],{expected_path:false});
add('holdout','S2','positive','Как внешний провайдер получает ключ из настроек?',path('api_key_name','хранилище ОС'),[
 ['secrets','пользователь — `api_key_name` из настроек','architecture'],['external','Ключ уходит заголовком `Authorization: Bearer <ключ>`','behavior']],
 {expectation:'Цепочка: имя записи → хранилище ОС → Authorization; каждый переход требует собственного основания.'});
add('holdout','S2','conditional','Когда ключ окружения заменяет ключ из хранилища?',rel('api_key_env'),[
 ['secrets','Если переменная задана, её значение используется вместо ключа из хранилища ОС','constraint']],
 {facts:[{subject:'api_key_env',predicate:'supersedes',object:'ключ из хранилища ОС',polarity:'positive',condition:'переменная задана'}]});

add('dev','S3','positive','Собери основания перехода Qwen на GGUF',ctx('Qwen GGUF'),[
 ['gguf','Автор выбрал встроенный llama.cpp и Q4_K_M','decision'],['local','Контекст создаётся лениво','behavior'],['gguf','macOS/Metal и другие GPU требуют отдельной приёмки','constraint']]);
add('dev','S3','negative','Есть ли подтверждённая полная приёмка Qwen 4B?',ctx('Qwen 4B приёмка'),[
 ['gguf','полное качество и 4B ещё принимаются','constraint']],{forbidden:'Успешная сборка не является полной приёмкой.'});
add('holdout','S3','positive','Собери правила безопасной установки bars в клиент',ctx('установка клиент профиль'),[
 ['clients','`install_env` по умолчанию `None`','constraint'],['clients','Перед правкой bars сохраняет копию файла','behavior'],['secrets','сам ключ — никогда','constraint']]);
add('holdout','S3','conditional','Что доступно до одобрения хука Codex?',ctx('Codex одобрение хука MCP'),[
 ['clients','Пока хук не одобрен, MCP работает','behavior'],['clients','одобрение привязано к хешу записи','constraint']]);

add('dev','S4','positive','Нужна ли переиндексация при изменении docs_root?',rel('Согласие'),[
 ['updates','Согласие пользователя не нужно: модель не менялась','constraint']],
 {facts:[{subject:'изменение docs_root',predicate:'requires',object:'согласие пользователя',polarity:'negative',condition:'модель не менялась'}]});
add('dev','S4','negative','Может ли агент MCP дать согласие на смену модели?',rel('агент'),[
 ['consent','агент через MCP согласие дать не может','constraint']],
 {facts:[{subject:'агент MCP',predicate:'uses',object:'согласие на смену модели',polarity:'negative'}]});
add('holdout','S4','positive','Требуется ли ключ для локального адреса модели?',rel('ключ'),[
 ['external','Для локального адреса ключ не нужен','constraint']],
 {facts:[{subject:'локальный адрес',predicate:'requires',object:'ключ',polarity:'negative'}]});
add('holdout','S4','conditional','Какой адрес rerank пробуется после 404?',rel('Rerank'),[
 ['external','при ответе 404 — `<base_url>/v1/rerank`','constraint']],
 {facts:[{subject:'rerank',predicate:'uses',object:'/v1/rerank',polarity:'positive',condition:'ответ 404'}]});

add('dev','S5','positive','Почему встроенный Qwen переведён с candle на llama.cpp?',ctx('Qwen candle llama.cpp решение'),[
 ['gguf','Candle загружает Qwen в FP32 на CPU','decision'],['gguf','Старый candle-путь Qwen удаляется при внедрении','decision']],{decision_status:'accepted architecture, product acceptance open'});
add('dev','S5','negative','Был ли выбран отдельный llama-server для встроенного Qwen?',ctx('llama-server встроенный Qwen решение'),[
 ['gguf','Отдельный llama-server — не соответствует решению','decision']],{forbidden:'Отклонённый вариант нельзя возвращать как текущее решение.'});
add('holdout','S5','positive','Почему документация bars разложена по фичам?',ctx('документация по фичам решение'),[
 ['layout','Автору так неудобно','decision'],['layout','Фичи — `docs/features/<name>/`','decision']],{decision_status:'принято'});
add('holdout','S5','conditional','Закрывается ли новая тема автоматически после появления документа?',ctx('новая тема закрытие решение пользователя'),[
 ['topics','закрытие только по действию пользователя','verification']],{forbidden:'Наличие документа не является решением пользователя о закрытии.'});

add('dev','S6','positive','Какие ограничения моделей и индексации документированы?',explore('модел'),[
 ['local','Длина входа','constraint'],['updates','Полная переиндексация','behavior'],['search','Реранк кандидатов','behavior']]);
add('dev','S6','negative','Документирована ли отдельная тема обучения собственной модели?',explore('обучение собственной модели'),[],{expected_empty:true});
add('holdout','S6','positive','Какие направления жизненного цикла проектов и интеграций описаны?',{operation:'explore',whole_scope:true,analysis:'topics'},[
 ['projects','Полное удаление данных проекта','behavior'],['clients','Как bars правит чужие конфиги','behavior'],['topics','Управление','requirement']]);
add('holdout','S6','conditional','Какие ограничения внешних провайдеров описаны?',explore('провайдер'),[
 ['external','Размерность внешней модели','behavior'],['secrets','Переопределение ключа через окружение','behavior']]);

add('dev','S7','positive','Как размер чанков влияет на совместимость поиска после смены параметров?',path('параметры нарезки','отпечаток модели'),[
 ['updates','векторы чанков, текст которых не изменился, берутся из индекса по хешу','behavior'],['local','Префикс документа входит в отпечаток модели','behavior']]);
add('dev','S7','negative','Доказывает ли высокий косинус истинность результата?',rel('оценка'),[
 ['search','Это не вероятность правильного ответа','constraint']],{forbidden:'Косинус не доказывает предикат зависимости.'});
add('holdout','S7','positive','Как одобрение хука связано с регистрацией проекта и доступом MCP?',path('хук','MCP'),[
 ['clients','Пока хук не одобрен, MCP работает, а проект регистрируется окном или CLI','behavior'],['projects','Т-6','requirement']]);
add('holdout','S7','conditional','Почему неподдерживаемая размерность ответа останавливает поиск?',rel('размерность'),[
 ['external','индексация и поиск останавливаются ошибкой о размерности','constraint']],{expectation:'Условие — тот же адрес вернул вектор другого размера; не переносить на неизвестную размерность 0.'});

add('dev','S8','positive','Есть ли противоречивые описания доступности поиска при перестройке?',{operation:'explore',whole_scope:true,analysis:'conflict_candidates'},[
 ['consent','пока идёт переиндексация, поиск по проекту не работает','behavior'],['updates','Полный поиск с SQLite/rerank','verification']],{expectation:'Кандидат расхождения, требуется различить смену модели и полную перестройку прежней моделью.'});
add('dev','S8','negative','Можно ли считать отсутствие извлечённой проверки доказанным отсутствием тестов?',{operation:'explore',whole_scope:true,analysis:'coverage'},[],{expected_semantic_partial:true});
add('holdout','S8','positive','Какие ссылки проекта не разрешаются в выбранном снимке?',{operation:'explore',whole_scope:true,analysis:'connectivity'},[],{audit:'Каждый broken-link проверить по frozen sources и списку исключений; отсутствующий в снимке файл не объявлять отсутствующим в репозитории.'});
add('holdout','S8','conditional','Как отличить описанную проверку установки от нативной приёмки?',{operation:'explore',topic:'установка',analysis:'coverage'},[
 ['clients','Как проверяется и что не подтверждено','verification']],{expected_semantic_partial:true});

add('dev','control','exact-1','rerank_candidates',ctx('rerank_candidates'),[
 ['search','`rerank_candidates`, по умолчанию 15','behavior']]);
add('dev','control','exact-2','accept_model_change',ctx('accept_model_change'),[
 ['consent','окно вызывает `reindex` с `accept_model_change`','behavior']]);
add('holdout','control','exact-1','api_key_env',ctx('api_key_env'),[
 ['secrets','У провайдера в настройках может быть поле `api_key_env`','behavior']]);
add('holdout','control','exact-2','project_delete',ctx('project_delete'),[
 ['deletion','Новый метод `project_delete` доступен только окну','behavior']]);

const dev=new Set(cases.filter(c=>c.split==='dev').flatMap(c=>c.evidence.map(e=>e.path)));
const holdout=new Set(cases.filter(c=>c.split==='holdout').flatMap(c=>c.evidence.map(e=>e.path)));
if([...dev].some(p=>holdout.has(p))) throw Error('document split leaks');
fs.writeFileSync(output,JSON.stringify({format:1,created_at:new Date().toISOString(),budget_bytes:16384,
  reviewer:'single agent; annotations before current outputs; not independent human review',
  cases},null,2),{flag:'wx'});
console.log(JSON.stringify({cases:cases.length,development_documents:dev.size,holdout_documents:holdout.size,
  sha256:crypto.createHash('sha256').update(fs.readFileSync(output)).digest('hex')}));
