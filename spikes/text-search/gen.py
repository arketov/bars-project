import os, random, sys, glob
src, out, n = sys.argv[1], sys.argv[2], int(sys.argv[3])
random.seed(7)
lines = []
for f in glob.glob(os.path.join(src, "**", "*.md"), recursive=True):
    if f.endswith("BARS_INSTRUCTION.md"):
        continue
    try:
        lines += [l for l in open(f, encoding="utf-8").read().split("\n") if l.strip()]
    except Exception:
        pass
for i in range(n):
    d = os.path.join(out, f"m{i // 5000:02d}", f"f{(i // 100) % 50:02d}")
    os.makedirs(d, exist_ok=True)
    body, size = [f"# Документ {i}", ""], 0
    while size < 8000:
        l = random.choice(lines)
        if random.random() < 0.001:
            l += " zqxrare"
        body.append(l)
        size += len(l.encode()) + 1
    open(os.path.join(d, f"doc{i}.md"), "w", encoding="utf-8", newline="\n").write("\n".join(body) + "\n")
