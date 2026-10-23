// Согласованная копия SQLite с учётом WAL; исходная БД открыта только на чтение.
import { DatabaseSync, backup } from 'node:sqlite';
import fs from 'node:fs';
import path from 'node:path';
const [source, destination] = process.argv.slice(2);
if (!source || !destination || fs.existsSync(destination)) throw Error('Нужны исходная БД и новый файл снимка');
fs.mkdirSync(path.dirname(destination), { recursive: true });
const db = new DatabaseSync(source, { readOnly: true });
try { await backup(db, destination); } finally { db.close(); }
console.log(JSON.stringify({ snapshot: destination, bytes: fs.statSync(destination).size }));
