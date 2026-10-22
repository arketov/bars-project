import sqlite3,json,sys
c=sqlite3.connect('file:.bars/index.sqlite?mode=ro',uri=True)
src=dict(c.execute("select node_id,source_path from graph_node_sources"))
docs=sorted(p for (p,) in c.execute("select path from graph_sources"))
ix={p:i for i,p in enumerate(docs)}
pairs=set()
for f,t in c.execute("select from_id,to_id from graph_edges where predicate='\"links_to\"'"):
    a,b=src.get(f),src.get(t)
    if a in ix and b in ix and a!=b: pairs.add((ix[a],ix[b]))
json.dump({"docs":docs,"links":sorted(pairs)},open(sys.argv[1],'w',encoding='utf-8'),ensure_ascii=False)
print(len(docs),len(pairs))
