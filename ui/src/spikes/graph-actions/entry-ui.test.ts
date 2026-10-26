import { afterEach, expect, it } from 'vitest';
import { mount, tick, unmount } from 'svelte';
import PhysicalGraph from './PhysicalGraph.svelte';
import { graphEntry } from './entry-state.svelte';

const components: ReturnType<typeof mount>[] = [];
const targets: HTMLElement[] = [];
function panel(mini = false) {
  const target = document.createElement('div');
  if (mini) target.className = 'mini';
  document.body.append(target);
  targets.push(target);
  components.push(mount(PhysicalGraph, { target }));
  return target;
}
afterEach(async () => {
  for (const component of components.splice(0)) await unmount(component);
  for (const target of targets.splice(0)) target.remove();
  graphEntry.query = null;
});

it('сохраняет полноценную страницу с поиском и не предлагает общий список узлов', async () => {
  const full = panel();
  await tick();
  expect(full.querySelector('h1')?.textContent).toBe('Граф документации');
  expect(full.querySelector('[aria-label="Задача или тема графа"]')).not.toBeNull();
  expect(full.querySelector('button[aria-label="Показать списком"]')).toBeNull();
  expect(full.querySelector('[aria-label="Узлы графа"]')).toBeNull();
  const help = [...full.querySelectorAll('details')].find(d => d.textContent?.includes('Как управлять картой'));
  expect(help).toBeDefined();
  expect(help?.open).toBe(false);
});

it('инструменты принадлежат карте и присутствуют у мини-графа', async () => {
  const full = panel();
  const mini = panel(true);
  await tick();
  for (const target of [full, mini]) {
    const tools = target.querySelector('[role="toolbar"][aria-label^="Инструменты"]');
    expect(tools?.closest('.stage')).not.toBeNull();
    expect(tools?.querySelector('[aria-label="Вписать"]')).not.toBeNull();
    expect(tools?.querySelector('[aria-label="Настройки графа"]')).not.toBeNull();
  }
});

it('поиск на странице показывает завершённый запрос в обеих картах', async () => {
  const full = panel();
  const mini = panel(true);
  await tick();
  const input = full.querySelector<HTMLInputElement>('[aria-label="Задача или тема графа"]')!;
  input.value = 'индекс';
  input.dispatchEvent(new Event('input', { bubbles: true }));
  full.querySelector('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
  await tick();
  await tick();
  expect(full.querySelector('[aria-label="Вид запроса"]')?.textContent).toContain('индекс');
  expect(mini.querySelector('.qplate')?.textContent).toContain('индекс');
  [...full.querySelectorAll<HTMLButtonElement>('button')].find(b => b.textContent === 'Сменить тему')!.click();
  await tick();
  expect(full.querySelector('[aria-label="Вид запроса"]')?.textContent).toContain('индекс');
  expect(mini.querySelector('.qplate')?.textContent).toContain('индекс');
});
