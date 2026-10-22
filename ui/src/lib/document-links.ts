// Ссылки документа из индекса ссылок воркера (`links_of`): читает диск, не зависит от графа и модели.
import { call } from "./api";

export interface OutgoingLink {
  line: number;
  /** Видимый текст ссылки. */
  text: string;
  /** Адресат как написан в документе. */
  raw: string;
  kind: "inline" | "reference" | "wiki";
  /** Путь документа-адресата от корня проекта; нет у битой ссылки. */
  target?: string;
  target_title?: string;
  broken: boolean;
}

export interface IncomingLink {
  /** Путь документа-источника от корня проекта. */
  path: string;
  title?: string;
  /** Строки источника, где стоит ссылка. */
  lines: number[];
}

export interface DocumentLinks {
  known: boolean;
  outgoing: OutgoingLink[];
  incoming: IncomingLink[];
}

export const loadLinks = (project: string, path: string) => call<DocumentLinks>("links_of", { project, path });

/** Название для списка: заголовок из frontmatter или имя файла без `.md`. */
export const linkTitle = (title: string | undefined, path: string) => title || (path.split("/").pop() ?? path).replace(/\.md$/i, "");
