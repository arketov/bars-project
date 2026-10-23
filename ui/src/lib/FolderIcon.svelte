<script lang="ts">
  // Иконки папок Т-38: линейные, цвет берут у текста (задаётся снаружи), как Icon.svelte.
  import type { IconId } from "./appearance";

  const PATHS: Record<IconId, string> = {
    folder: "M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z",
    book: "M5 4h10a3 3 0 0 1 3 3v13H8a3 3 0 0 1-3-3zM5 17a3 3 0 0 1 3-3h10M9 8h5",
    gear: "M12 9a3 3 0 1 0 0 6 3 3 0 0 0 0-6zM12 3v3M12 18v3M3 12h3M18 12h3M5.6 5.6l2.1 2.1M16.3 16.3l2.1 2.1M18.4 5.6l-2.1 2.1M7.7 16.3l-2.1 2.1",
    code: "M9 7l-5 5 5 5M15 7l5 5-5 5M13.5 5l-3 14",
    flask: "M9 3h6M10 3v6l-5 9a2 2 0 0 0 1.8 3h10.4a2 2 0 0 0 1.8-3l-5-9V3M8 15h8",
    chat: "M4 5h16v11H9l-5 4zM8 9h8M8 12h5",
    star: "M12 3l2.8 5.7 6.2.9-4.5 4.4 1.1 6.2L12 17.3 6.4 20.2l1.1-6.2L3 9.6l6.2-.9z",
    bolt: "M13 3L5 14h6l-1 7 8-11h-6z",
    shield: "M12 3l7 3v5c0 5-3 8-7 10-4-2-7-5-7-10V6zM9 12l2 2 4-4",
    chart: "M4 20V4M4 20h16M8 16v-4M12 16V8M16 16v-6",
    database: "M5 6c0-1.7 3.1-3 7-3s7 1.3 7 3-3.1 3-7 3-7-1.3-7-3zM5 6v6c0 1.7 3.1 3 7 3s7-1.3 7-3V6M5 12v6c0 1.7 3.1 3 7 3s7-1.3 7-3v-6",
    globe: "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM3 12h18M12 3c3 3 3 15 0 18M12 3c-3 3-3 15 0 18",
    users: "M9 11a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM3 20c0-3.3 2.7-6 6-6s6 2.7 6 6M16 5.2a3 3 0 0 1 0 5.6M18 14.3c1.8.8 3 2.6 3 4.7",
    bug: "M9 9a3 3 0 0 1 6 0v8a3 3 0 0 1-6 0zM9 12H4M15 12h5M9 16l-4 2M15 16l4 2M9 8L6 6M15 8l3-2M12 9v10",
    target: "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM12 7a5 5 0 1 0 0 10 5 5 0 0 0 0-10zM12 11a1 1 0 1 0 0 2 1 1 0 0 0 0-2z",
    rocket: "M14 4c3 0 6 3 6 6-2 4-5 7-9 8l-4-4c1-4 4-7 7-10zM9 14l-4 1 1-4M10 18l-1 4 4-1M15 8.5a1 1 0 1 0 0 .01",
    puzzle: "M5 8h4a2 2 0 1 1 4 0h4v4a2 2 0 1 1 0 4v4H5v-4a2 2 0 1 0 0-4z",
    archive: "M4 5h16v4H4zM6 9v10h12V9M10 13h4",
    pencil: "M4 20l1-5L16 4l4 4L9 19zM14 6l4 4",
    magnifier: "M11 4a7 7 0 1 0 0 14 7 7 0 0 0 0-14zM20 20l-4-4",
  };

  let { name = null, size = 14, width = 2 }: { name?: IconId | null; size?: number; width?: number } = $props();
  const path = $derived(PATHS[name ?? "folder"]);
</script>

<svg viewBox="0 0 24 24" width={size} height={size} fill="none" stroke="currentColor" stroke-width={width} stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false"><path d={path} /></svg>
