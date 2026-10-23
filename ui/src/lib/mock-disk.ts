// Диск для просмотра окна в браузере (вне Tauri): те же операции с файлами и папками, что у команд
// `tree_read`/`entry_*`, но над копией документов мока в памяти. В тестах выключен: дерево строится по индексу.

import type { DiskTreeOutput, EntryResult } from "./api";
import { mockCall } from "./mock";
import type { TreeOutput } from "./types";

interface Disk {
  docsRoot: string;
  files: Set<string>;
  dirs: Set<string>;
  revision: number;
}

const disks = new Map<string, Disk>();
const enabled = () => import.meta.env.MODE !== "test";

async function diskOf(project: string): Promise<Disk> {
  let disk = disks.get(project);
  if (!disk) {
    const tree = (await mockCall("tree", { project })) as TreeOutput;
    disk = { docsRoot: tree.docs_root, files: new Set(), dirs: new Set(), revision: 1 };
    for (const document of tree.documents) {
      disk.files.add(document.path);
      const parts = document.path.split("/");
      for (let i = 1; i < parts.length; i++) disk.dirs.add(parts.slice(0, i).join("/"));
    }
    disks.set(project, disk);
  }
  return disk;
}

export async function mockTreeRead(project: string): Promise<DiskTreeOutput | null> {
  if (!enabled()) return null;
  const disk = await diskOf(project);
  const prefix = disk.docsRoot === "." ? "" : disk.docsRoot;
  return {
    status: "ok",
    docs_root: disk.docsRoot,
    revision: String(disk.revision),
    dirs: [...disk.dirs].filter((dir) => dir !== prefix && (!prefix || dir.startsWith(prefix + "/"))).sort(),
    files: [...disk.files].sort().map((path) => ({ path, size: 0, modified: 0 })),
    problems: [],
  };
}

const failure = (kind: string, message: string): EntryResult => ({ status: "error", kind, message });
const base = (path: string) => path.slice(path.lastIndexOf("/") + 1);
const join = (dir: string, name: string) => (dir ? `${dir}/${name}` : name);

function checkName(name: string, dir: boolean): string | EntryResult {
  if (!name) return failure("invalid_name", "Недопустимое имя: имя пустое");
  if (/[\\/:*?"<>|\u0000-\u001f]/.test(name)) return failure("invalid_name", "Недопустимое имя: знак запрещён");
  if (/[ .]$/.test(name)) return failure("invalid_name", "Недопустимое имя: имя не может оканчиваться пробелом или точкой");
  if (dir) return name.startsWith(".") ? failure("invalid_name", "Недопустимое имя: папка с точкой в начале невидима для индекса") : name;
  return /\.md$/i.test(name) ? name : `${name}.md`;
}

export async function mockEntry(op: string, project: string, args: Record<string, string>): Promise<EntryResult> {
  if (!enabled()) return failure("io", "Операции с файлами доступны в приложении bars");
  const disk = await diskOf(project);
  const clash = (dir: string, name: string, except?: string) =>
    [...disk.files, ...disk.dirs].find((path) => path !== except && path.toLowerCase() === join(dir, name).toLowerCase());
  const inside = (path: string) => [...disk.files, ...disk.dirs].filter((node) => node === path || node.startsWith(path + "/"));
  disk.revision++;
  if (op === "create_doc" || op === "create_dir") {
    const dir = op === "create_dir";
    const name = checkName(args.name, dir);
    if (typeof name !== "string") return name;
    const found = clash(args.parent, name);
    if (found) return failure("exists", `«${base(found)}» уже есть в этой папке`);
    const path = join(args.parent, name);
    if (dir) {
      disk.dirs.add(path);
      disk.files.add(join(path, "index.md"));
      return { status: "ok", path, index_path: join(path, "index.md") };
    }
    disk.files.add(path);
    return { status: "ok", path };
  }
  if (op === "move") {
    const isDir = disk.dirs.has(args.from);
    const name = checkName(args.name, isDir);
    if (typeof name !== "string") return name;
    if (isDir && (args.toParent === args.from || args.toParent.startsWith(args.from + "/"))) {
      return failure("invalid_move", "Так переместить нельзя: папку нельзя поместить внутрь неё самой");
    }
    const target = join(args.toParent, name);
    if (target === args.from) return failure("invalid_move", "Так переместить нельзя: имя и папка не изменились");
    const found = clash(args.toParent, name, args.from);
    if (found) return failure("exists", `«${base(found)}» уже есть в этой папке`);
    for (const node of inside(args.from)) {
      const moved = target + node.slice(args.from.length);
      if (disk.files.delete(node)) disk.files.add(moved);
      else if (disk.dirs.delete(node)) disk.dirs.add(moved);
    }
    return { status: "ok", path: target };
  }
  if (op === "trash") {
    for (const node of inside(args.path)) {
      disk.files.delete(node);
      disk.dirs.delete(node);
    }
    return { status: "ok", path: args.path };
  }
  return failure("io", `Неизвестная операция ${op}`);
}
