/**
 * Migration ordering. Repositories that are depended on must migrate before
 * their dependents. The plan groups repositories into waves: every repository
 * in a wave depends only on repositories in earlier waves, so a wave can be
 * delegated in parallel.
 */

export interface DependencyEdge {
  /** Repository that consumes `to`. */
  from: string;
  /** Repository that must migrate first. */
  to: string;
}

export type WavePlan =
  | { ok: true; waves: string[][] }
  | { ok: false; cycle: string[] };

export function planWaves(repositories: readonly string[], edges: readonly DependencyEdge[]): WavePlan {
  const nodes = [...new Set(repositories)].sort();
  const known = new Set(nodes);
  const blockers = new Map<string, Set<string>>(nodes.map((n) => [n, new Set<string>()]));
  const dependents = new Map<string, Set<string>>(nodes.map((n) => [n, new Set<string>()]));

  for (const { from, to } of edges) {
    if (!known.has(from) || !known.has(to)) {
      throw new Error(`Edge ${from} -> ${to} references an unknown repository`);
    }
    if (from === to) return { ok: false, cycle: [from, from] };
    blockers.get(from)?.add(to);
    dependents.get(to)?.add(from);
  }

  const remaining = new Map(nodes.map((n) => [n, blockers.get(n)?.size ?? 0]));
  const waves: string[][] = [];
  let ready = nodes.filter((n) => remaining.get(n) === 0);

  while (ready.length > 0) {
    waves.push(ready);
    const next: string[] = [];
    for (const done of ready) {
      remaining.delete(done);
      for (const dependent of dependents.get(done) ?? []) {
        const count = (remaining.get(dependent) ?? 0) - 1;
        remaining.set(dependent, count);
        if (count === 0) next.push(dependent);
      }
    }
    ready = next.sort();
  }

  if (remaining.size > 0) {
    return { ok: false, cycle: findCycle([...remaining.keys()], blockers) };
  }
  return { ok: true, waves };
}

function findCycle(candidates: string[], blockers: Map<string, Set<string>>): string[] {
  const inCandidates = new Set(candidates);
  const visiting: string[] = [];
  const onStack = new Set<string>();
  const finished = new Set<string>();

  const visit = (node: string): string[] | null => {
    visiting.push(node);
    onStack.add(node);
    for (const next of [...(blockers.get(node) ?? [])].sort()) {
      if (!inCandidates.has(next) || finished.has(next)) continue;
      if (onStack.has(next)) return [...visiting.slice(visiting.indexOf(next)), next];
      const found = visit(next);
      if (found) return found;
    }
    visiting.pop();
    onStack.delete(node);
    finished.add(node);
    return null;
  };

  for (const start of candidates.sort()) {
    if (finished.has(start)) continue;
    const found = visit(start);
    if (found) return found;
  }
  return candidates;
}
