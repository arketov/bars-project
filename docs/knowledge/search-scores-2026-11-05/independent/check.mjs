// Сверка своего конвейера с results.md основного сеанса.
import { load, ranks, summ, fmt, falseShare } from './common.mjs';
const Q = load();
const T = (s) => Q.main.filter((q) => q.meta.split === s);
const show = (name, cfg) => console.log(name.padEnd(28), fmt(summ(ranks(Q.main, cfg))), '|', fmt(summ(ranks(T('tune'), cfg))), '|', fmt(summ(ranks(T('test'), cfg))), '| ложн', (100 * falseShare(Q.neg, cfg)).toFixed(0));
console.log('main', Q.main.length, 'neg', Q.neg.length, 'ident', Q.main.filter((q) => q.meta.ident).map((q) => q.meta.id + ':' + q.meta.group).join(' '));
show('RRF+rr θ0.1', { rrfBase: true, rerank: true, K: 15, theta: 0.1 });
show('RRF+rr θ0', { rrfBase: true, rerank: true, K: 15, theta: 0 });
show('start θ0.1', { rerank: true, K: 15, wc: 0, wf: 0.1, wp: 0.1, theta: 0.1 });
show('start θ0', { rerank: true, K: 15, wc: 0, wf: 0.1, wp: 0.1, theta: 0 });
show('K30', { rerank: true, K: 30, wc: 0, wf: 0.1, wp: 0.1, theta: 0.1 });
show('RRF nr', { rrfBase: true, rerank: false, theta: 0 });
show('start nr θ0', { rerank: false, wf: 0.1, wp: 0.1, theta: 0 });
show('start nr θ0.1', { rerank: false, wf: 0.1, wp: 0.1, theta: 0.1 });
