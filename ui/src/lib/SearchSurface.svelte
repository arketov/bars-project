<script lang="ts">
  import { app } from './app.svelte';
  import SearchPane from './SearchPane.svelte';
  import WorkerDown from './WorkerDown.svelte';
  import EmptyProjects from './EmptyProjects.svelte';
  import Missing from './Missing.svelte';
  import NoDocs from './NoDocs.svelte';
  const project = $derived(app.project);
</script>

{#if app.workerDown}<WorkerDown message={app.workerDown} />
{:else if app.projectsLoaded && app.projects.length === 0}<EmptyProjects />
{:else if !project}<p role="status">Выберите проект</p>
{:else if project.state === 'unavailable'}<Missing {project} />
{:else if project.state === 'ready' && app.documentsOf(project) === 0 && app.tree.length === 0 && app.progressOf(project.path) === null && app.waitingOf(project.path) === 0}<NoDocs {project} />
{:else}<SearchPane />{/if}
