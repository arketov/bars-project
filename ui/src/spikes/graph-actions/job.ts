// Mockup transitions only; the worker confirms stop independently in production.
export type Job = 'idle' | 'running' | 'stopping' | 'stopped' | 'failed';
export function jobActions(job: Job, needsUpdate = false) {
  return { update: needsUpdate && job === 'idle' || job === 'stopped' || job === 'failed', stop: job === 'running' };
}
export const JOB_TEXT: Record<Job, string> = {
  idle: 'Явные ссылки актуальны. Полнота смысловых отношений неизвестна.',
  running: 'Обновляем связи… Прежняя карта доступна.',
  stopping: 'Остановка запрошена. Ожидаем подтверждение.',
  stopped: 'Построение остановлено. Новые изменения будут обновляться автоматически.',
  failed: 'Не удалось обновить связи. Прежняя карта сохранена.',
};
