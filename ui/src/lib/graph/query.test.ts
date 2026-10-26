import { expect, it } from "vitest";
import { acceptQuerySlice, queryRequest, shapeQuery, type QueryInput } from "./query";
import { queryFixture } from "../mock-query";
const input: QueryInput = { id: "q", text: "индексация", hits: [{path:"docs/a.md",module:".",title:"А",heading_chain:"А",first_line:2,last_line:4,text:"текст",score:1.2,score_kind:"rerank"}],scope:{modules:[],files:[]},scoreKind:"rerank",requested:"hybrid",applied:"hybrid",minScore:0.1 };
const docs = [{path:"docs/a.md",module:".",title:"А",chunks:1},{path:"docs/b.md",module:".",title:"Б",chunks:1}];
it("отправляет компактную выдачу и её область без embedding, текста чанка и смены оценки", () => {
  const r = queryRequest(input, 2, .8);
  expect(r).toMatchObject({depth:2,fanout:4,score_kind:"rerank",max_nodes:200,max_edges:800,max_bytes:262144});
  expect(r.hits[0]).toEqual({source_hash:"",channel:"primary",path:"docs/a.md",title:"А",heading:"А",first:2,last:4,score:1.2});
  expect(queryRequest({...input, scoreKind:"none"},0,.5).hits[0].score).toBeNull();
});
it("отображает Rust-индексы и абсолютную оценку; клиент не достраивает окружение", () => {
  const server = queryFixture({request:queryRequest(input,1,.5)},docs);
  server.views.all = [0];
  const {slice,info} = acceptQuerySlice(server,input);
  expect(slice.edges.find(e=>e.kind==="query_result")).toMatchObject({raw:1.2,s:1,scoreKind:"rerank"});
  expect(shapeQuery(slice,info,"all").nodes).toEqual([info.queryNode,0]);
  expect(info.results[0].fragments[0]).toMatchObject({first:2,last:4});
});
it("нейтральная связь без оценки и результат вне снимка остаются честными", () => {
  const q = {...input,scoreKind:"none" as const};
  const server = queryFixture({request:queryRequest(q,0,.5)},docs);
  const {slice} = acceptQuerySlice(server,q);
  expect(slice.edges.find(e=>e.kind==="query_result")).toMatchObject({raw:null,neutral:true});
  server.results[0].node = null;
  server.results[0].status = "no_graph_object";
  server.query_edges = [];
  const missing = acceptQuerySlice(server,q);
  expect(missing.info.results[0]).toMatchObject({node:null,status:"no_graph_object"});
  expect(missing.slice.edges.some(e=>e.kind==="query_result")).toBe(false);
});
it("чужой ответ поиска отвергается", () => {
  const server = queryFixture({request:queryRequest(input,0,.5)},docs);
  server.id = "старый";
  expect(()=>acceptQuerySlice(server,input)).toThrow("другому поиску");
});

it("связанные источники сохраняют хеш и не получают оценки запроса", () => {
  const bundle = { sources: [{ path:"docs/b.md",title:"Б",hash:"revision-b" }] } as QueryInput["bundle"];
  const r = queryRequest({...input,bundle,applied:"mix"},1,.5);
  expect(r.hits[1]).toMatchObject({source_hash:"revision-b",channel:"related",score:null});
});
it("сохраняет статус и пунктирную роль результата из Rust", () => {
  const server = queryFixture({request:queryRequest(input,1,.5)},docs);
  server.results[0].status = "deleted_since_search";
  const {slice,info} = acceptQuerySlice(server,input);
  expect(info.results[0].status).toBe("deleted_since_search");
  expect(slice.nodes[info.results[0].node!].ghost).toBe(true);
});
