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

export type WavePlan = { ok: true; waves: string[][] } | { ok: false; cycle: string[] };

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

// ---------------------------------------------------------------------------
// Campaign rollout planning (Task 4). Pure and deterministic: same evidence in,
// same plan out. Every claim cites the fact or state it came from; inferred
// edges are labelled, never presented as certain.

export interface EvidenceFact {
  category: string;
  key: string;
  value: string;
  file?: string | null;
  line?: number | null;
  pointer?: string | null;
  method?: string;
  confidence?: number;
}

export interface MemberEvidence {
  repositoryId: string;
  name: string;
  trustStatus: string;
  /** `external`, `shiftops_managed` or null for metadata-only records. */
  ownership: string | null;
  hasLocalCheckout: boolean;
  /** Uncommitted change count from the last read-only Git inspection. */
  uncommittedChanges: number | null;
  /** Timestamp of the latest completed intelligence scan, if any. */
  lastScannedAt: string | null;
  facts: EvidenceFact[];
  conflicts: { subject: string; message: string }[];
}

export interface PlanEdge {
  /** Member that depends on `to`. */
  from: string;
  /** Member (or non-member repository id) that must migrate first. */
  to: string;
  confidence: number;
  confirmationStatus: "inferred" | "confirmed" | "rejected";
  detectionMethod: string;
}

export interface MigrationGoal {
  /** Runtime fact key the goal targets, e.g. `node`. Empty = goal-agnostic. */
  runtime?: string | undefined;
  /** Major version the migration starts from, e.g. `18`. */
  sourceVersion?: string | undefined;
  /** Major version the migration targets, e.g. `22`. */
  targetVersion?: string | undefined;
}

export type PlanClassification = "affected" | "not_affected" | "needs_review";

export interface PlannedMember {
  repositoryId: string;
  name: string;
  classification: PlanClassification;
  /** 0–1, higher = more risk. null when blocked before scoring. */
  riskScore: number | null;
  /** Evidence-cited explanations, stable order. */
  reasons: string[];
  /** 0 = blocked before waves; 1..n = rollout wave. */
  waveNumber: number;
  /** Human sign-off required before delegation. */
  reviewRequired: boolean;
  blockedReason: string | null;
}

export interface UnknownEdge {
  from: string;
  to: string;
  detectionMethod: string;
}

export interface CampaignPlan {
  members: PlannedMember[];
  /** Edges actually used for ordering (both ends are campaign members). */
  usedEdges: PlanEdge[];
  /** Edges pointing at repositories outside the campaign. */
  unknownEdges: UnknownEdge[];
  /** Repositories each member blocks from starting (its campaign dependents). */
  dependents: Record<string, string[]>;
  cycle: string[] | null;
}

function majorOf(version: string): string | null {
  const m = version.match(/(\d+)/);
  return m ? (m[1] ?? null) : null;
}

function factCite(f: EvidenceFact): string {
  const where = f.pointer ? `${f.file ?? "?"}${f.pointer}` : f.line ? `${f.file ?? "?"}:${f.line}` : (f.file ?? "?");
  return `${f.key}=${f.value} (${where})`;
}

function classify(m: MemberEvidence, goal: MigrationGoal): { cls: PlanClassification; reasons: string[] } {
  const reasons: string[] = [];
  if (!goal.runtime) {
    if (m.facts.length === 0) {
      reasons.push("No scan facts recorded; cannot show the goal applies.");
      return { cls: "needs_review", reasons };
    }
    reasons.push(`${m.facts.length} scan facts recorded.`);
    return { cls: "affected", reasons };
  }
  const runtimeFacts = m.facts.filter((f) => f.category === "runtime" && f.key === goal.runtime);
  if (runtimeFacts.length === 0) {
    const anyRuntime = m.facts.some((f) => f.category === "runtime");
    if (anyRuntime) {
      reasons.push(`No ${goal.runtime} runtime declared; other runtimes present, so the goal does not apply.`);
      return { cls: "not_affected", reasons };
    }
    reasons.push(`No ${goal.runtime} runtime evidence found; the scan may be incomplete.`);
    return { cls: "needs_review", reasons };
  }
  const src = goal.sourceVersion ? majorOf(goal.sourceVersion) : null;
  const tgt = goal.targetVersion ? majorOf(goal.targetVersion) : null;
  const majors = runtimeFacts.map((f) => ({ f, major: majorOf(f.value) }));
  const onTarget = tgt !== null && majors.every((m) => m.major === tgt);
  const mixed = tgt !== null && majors.some((m) => m.major === tgt) && majors.some((m) => m.major !== tgt);
  let cls: PlanClassification = "affected";
  for (const { f, major } of majors) {
    if (tgt !== null && major === tgt) {
      reasons.push(`Already on target: ${factCite(f)}.`);
    } else if (src !== null && major === src) {
      reasons.push(`On source version: ${factCite(f)}.`);
    } else {
      reasons.push(
        `Declares ${factCite(f)}, which is neither source ${goal.sourceVersion ?? "?"} nor target ${goal.targetVersion ?? "?"}.`,
      );
    }
  }
  if (mixed) {
    cls = "needs_review";
    reasons.push("Conflicting runtime declarations; human review needed.");
  } else if (onTarget) {
    cls = "not_affected";
  }
  return { cls, reasons };
}

function risk(m: MemberEvidence, dependentCount: number): { score: number; reasons: string[] } {
  const reasons: string[] = [];
  let score = 0;
  if ((m.uncommittedChanges ?? 0) > 0) {
    score += 0.25;
    reasons.push(`${m.uncommittedChanges} uncommitted changes at last Git inspection.`);
  }
  const conflictScore = Math.min(0.3, m.conflicts.length * 0.1);
  if (conflictScore > 0) {
    score += conflictScore;
    reasons.push(`${m.conflicts.length} unresolved scan conflict(s): ${m.conflicts.map((c) => c.subject).join(", ")}.`);
  }
  const hasLockfile = m.facts.some((f) => f.category === "lockfile");
  const hasManifest = m.facts.some((f) => f.category === "manifest");
  if (hasManifest && !hasLockfile) {
    score += 0.1;
    reasons.push("Dependency manifest present but no lockfile; installs may drift.");
  }
  if (!m.facts.some((f) => f.category === "ci")) {
    score += 0.1;
    reasons.push("No CI configuration detected; no automated check gates the change.");
  }
  if (dependentCount >= 2) {
    score += 0.1;
    reasons.push(`${dependentCount} campaign repositories depend on this one; failure blocks them.`);
  }
  return { score: Math.min(1, Math.round(score * 100) / 100), reasons };
}

/**
 * Build the rollout plan. Wave 0 collects everything that cannot be planned:
 * blocked trust, no checkout, no completed scan, or membership in a dependency
 * cycle. Remaining members are ordered dependencies-before-dependents.
 */
export function planCampaign(
  members: readonly MemberEvidence[],
  edges: readonly PlanEdge[],
  goal: MigrationGoal,
): CampaignPlan {
  const ids = members.map((m) => m.repositoryId);
  const memberSet = new Set(ids);
  const byId = new Map(members.map((m) => [m.repositoryId, m]));

  const usedEdges: PlanEdge[] = [];
  const unknownEdges: UnknownEdge[] = [];
  const dependents: Record<string, string[]> = {};
  for (const id of ids) dependents[id] = [];
  for (const e of edges) {
    if (e.confirmationStatus === "rejected") continue;
    if (memberSet.has(e.from) && memberSet.has(e.to)) {
      usedEdges.push(e);
      dependents[e.to] = [...(dependents[e.to] ?? []), byId.get(e.from)?.name ?? e.from].sort();
    } else if (memberSet.has(e.from) || memberSet.has(e.to)) {
      unknownEdges.push({ from: e.from, to: e.to, detectionMethod: e.detectionMethod });
    }
  }
  const waveEdges: DependencyEdge[] = usedEdges.map((e) => ({ from: e.from, to: e.to }));
  const full = planWaves(ids, waveEdges);
  const cyclic = new Set(full.ok ? [] : full.cycle);
  // Plan only the acyclic remainder; cycle members are blocked at wave 0.
  const orderable = ids.filter((id) => !cyclic.has(id));
  const waves = planWaves(
    orderable,
    waveEdges.filter((e) => !cyclic.has(e.from) && !cyclic.has(e.to)),
  );

  const out: PlannedMember[] = members.map((m) => {
    const reasons: string[] = [];
    let blocked: string | null = null;
    if (m.trustStatus === "blocked") blocked = "Repository trust is blocked.";
    else if (!m.hasLocalCheckout) blocked = "No local checkout; delegation needs files on disk.";
    else if (!m.lastScannedAt) blocked = "No completed intelligence scan; run a scan first.";
    else if (cyclic.has(m.repositoryId)) blocked = "Member of a dependency cycle; resolve the cycle first.";
    if (blocked) {
      return {
        repositoryId: m.repositoryId,
        name: m.name,
        classification: "needs_review" as const,
        riskScore: null,
        reasons: [blocked],
        waveNumber: 0,
        reviewRequired: true,
        blockedReason: blocked,
      };
    }
    const { cls, reasons: clsReasons } = classify(m, goal);
    reasons.push(...clsReasons);
    const { score, reasons: riskReasons } = risk(m, dependents[m.repositoryId]?.length ?? 0);
    reasons.push(...riskReasons);
    const external = unknownEdges.some((e) => e.from === m.repositoryId && !memberSet.has(e.to));
    if (external) {
      reasons.push("Declares a dependency on a repository outside this campaign.");
    }
    const reviewRequired = cls === "needs_review" || score >= 0.5 || external;
    return {
      repositoryId: m.repositoryId,
      name: m.name,
      classification: cls,
      riskScore: score,
      reasons,
      waveNumber: 0,
      reviewRequired,
      blockedReason: null,
    };
  });

  for (const member of out) {
    if (member.blockedReason) continue;
    if (waves.ok) {
      const index = waves.waves.findIndex((w) => w.includes(member.repositoryId));
      member.waveNumber = index >= 0 ? index + 1 : 0;
    }
    if (member.waveNumber === 0) {
      // A second, disjoint cycle can keep the remainder unordered.
      member.blockedReason = "Unresolvable dependency ordering; review the graph.";
      member.reviewRequired = true;
    }
  }
  out.sort((a, b) => a.waveNumber - b.waveNumber || a.name.localeCompare(b.name));
  return {
    members: out,
    usedEdges,
    unknownEdges: unknownEdges.sort((a, b) => a.from.localeCompare(b.from) || a.to.localeCompare(b.to)),
    dependents,
    cycle: full.ok ? null : full.cycle,
  };
}
