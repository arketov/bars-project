<script lang="ts">
  // Значки мокапов (docs/features/window/mockups): линейные, цвет берут у текста.
  type Name =
    | "chevron-down" | "chevron-right" | "plus" | "search" | "moon" | "gear" | "arrow-right"
    | "external" | "close" | "doc" | "copy" | "back" | "folder" | "warn" | "info" | "error"
    | "pause" | "play" | "stop" | "check" | "refresh" | "bell" | "new-topics" | "more" | "eye" | "minus" | "fit" | "layout" | "branch";

  const PATHS: Record<Name, string> = {
    branch: "M6 3v12M18 9a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM6 21a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM18 9a9 9 0 0 1-9 9",
    minus: "M5 12h14",
    fit: "M8 3H3v5M16 3h5v5M3 16v5h5M21 16v5h-5",
    layout: "M5 4h4v4H5zM15 5h4v4h-4zM9 16h4v4H9zM9 6l6 1M7 8l4 8M17 9l-6 7",
    eye: "M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12zM15 12a3 3 0 1 1-6 0 3 3 0 0 1 6 0z",
    "new-topics": "M13 3H6a2 2 0 0 0-2 2v15a1 1 0 0 0 1 1h13a1 1 0 0 0 1-1v-8M8 11h4M8 15h7M8 18h4M18 2l1.2 3.8L23 7l-3.8 1.2L18 12l-1.2-3.8L13 7l3.8-1.2Z",
    more: "M5 12h.01M12 12h.01M19 12h.01",
    bell: "M18 8a6 6 0 0 0-12 0c0 7-3 7-3 9h18c0-2-3-2-3-9M10 21h4",
    "chevron-down": "m7 10 5 5 5-5",
    "chevron-right": "M3 1l4 4-4 4",
    plus: "M12 5v14M5 12h14",
    search: "M11 4a7 7 0 1 0 0 14 7 7 0 0 0 0-14zM20 20l-3.5-3.5",
    moon: "M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z",
    gear: "M12 9a3 3 0 1 0 0 6 3 3 0 0 0 0-6zM19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z",
    "arrow-right": "M5 12h14M13 6l6 6-6 6",
    external: "M14 3h7v7M10 14 21 3M21 14v7H3V3h7",
    close: "M18 6 6 18M6 6l12 12",
    doc: "M14 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9zM14 3v6h6M8 13h8M8 17h5",
    copy: "M11 9h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2h-8a2 2 0 0 1-2-2v-8a2 2 0 0 1 2-2zM5 15H4a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1h10a1 1 0 0 1 1 1v1",
    back: "M19 12H5M11 18l-6-6 6-6",
    folder: "M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z",
    warn: "M12 3 2 20h20zM12 10v4M12 17.5v.01",
    info: "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM12 11v5M12 8v.01",
    error: "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM9 9l6 6M15 9l-6 6",
    pause: "M8 5v14M16 5v14",
    play: "M7 4v16l13-8z",
    stop: "M6 6h12v12H6z",
    check: "m5 12 5 5 9-10",
    refresh: "M20 11a8 8 0 1 0-2.3 5.7M20 4v7h-7",
  };

  let { name, size = 15, width = 2.2 }: { name: Name; size?: number; width?: number } = $props();
</script>

<svg
  width={size}
  height={size}
  viewBox={name === "chevron-right" ? "0 0 10 10" : "0 0 24 24"}
  fill="none"
  stroke="currentColor"
  stroke-width={width}
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
>
  <path d={PATHS[name]} />
</svg>
