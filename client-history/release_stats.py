import subprocess, re, collections, sys
G = sys.argv[1]
def g(*a):
    return subprocess.run([G, *a], capture_output=True, text=True).stdout.strip()
drops = [12,13,14,16,17,18,19,20,21,22,23,24,25,26]
fork = {}; rev = {}; other = {}
for n in drops:
    b = f"release/2026_DROP{n}"
    fork[n] = g("merge-base", "dev", b)
    rev[n] = []; other[n] = []
    for line in g("log", "--reverse", "--format=%h|%ad|%s", "--date=short", f"dev..{b}").splitlines():
        h, d, s = line.split("|", 2)
        m = re.match(r"Revert \"Merge branch '([^']*)' into 'dev'\"", s)
        if m: rev[n].append(m.group(1))
        elif s.startswith("Revert"): rev[n].append(s[8:60])
        else: other[n].append((d, h, s[:70]))
tags = collections.defaultdict(list)
for line in g("for-each-ref", "--sort=creatordate", "--format=%(refname:short)|%(creatordate:short)", "refs/tags").splitlines():
    t, d = line.split("|")
    m = re.match(r"2026_DROP(\d+)\.(\d+)", t)
    if m: tags[int(m.group(1))].append((int(m.group(2)), d, t))
prev = None
print("drop | fork date | merges in window | distinct branches | reverted (distinct) | % held back | tags")
allmerged = {}
for n in drops:
    rng = f"{fork[prev]}..{fork[n]}" if prev else None
    if rng:
        subs = g("log", "--first-parent", "--merges", "--format=%s", rng).splitlines()
        br = [m.group(1) for s in subs if (m := re.match(r"Merge branch '([^']*)' into 'dev'", s))]
    else:
        br = []
    allmerged[n] = br
    fd = g("log", "-1", "--format=%ad", "--date=short", fork[n])
    dr = sorted(set(rev[n]))
    pct = f"{100*len(dr)/max(1,len(set(br))):.0f}%" if br else "-"
    print(f"DROP{n} | {fd} | {len(br)} | {len(set(br))} | {len(rev[n])} ({len(dr)}) | {pct} | " + ", ".join(f".{p} {d[5:]}" for p, d, t in tags[n]))
    prev = n
print()
# how often is each feature reverted, over which drops
where = collections.defaultdict(list)
for n in drops:
    for r in sorted(set(rev[n])): where[r].append(n)
multi = {k: v for k, v in where.items() if len(v) > 1}
print("reverted in more than one drop:")
for k, v in sorted(multi.items(), key=lambda x: -len(x[1])): print("  ", k, v)
print("distinct reverted items:", len(where), "| reverted once:", sum(1 for v in where.values() if len(v) == 1))
# same branch reverted multiple times inside one drop (merged several times)
print("merged-and-reverted several times within one drop:")
for n in drops:
    c = collections.Counter(rev[n])
    for k, v in c.items():
        if v > 1: print("  ", n, k, v)
# when did a reverted feature ship: first later drop where not reverted
idx = {n: i for i, n in enumerate(drops)}
wait = collections.Counter()
for k, v in where.items():
    last = max(v)
    i = idx[last] + 1
    wait[len(v)] += 1
print("number of drops a held-back item was held:", dict(wait))
print()
for n in drops:
    if other[n]:
        print(f"DROP{n} non-revert commits on release branch:")
        for d, h, s in other[n]: print("   ", d, h, s)
