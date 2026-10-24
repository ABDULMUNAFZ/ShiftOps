import { Card, EmptyState } from "@shiftops/ui";
import { Bot, FolderGit2, ShieldCheck, Workflow } from "lucide-react";
import { Page } from "./Page";

/*
 * Sections whose data sources are not implemented in this build. Each shows
 * a truthful empty state describing what will populate it, never sample data.
 */

export function RepositoriesPage() {
  return (
    <Page title="Repositories" description="Local and GitHub repositories that ShiftOps can migrate.">
      <Card>
        <EmptyState icon={<FolderGit2 />} title="No repositories">
          Repository discovery (local folders and GitHub organisations) is not implemented in this build. No
          repositories have been scanned.
        </EmptyState>
      </Card>
    </Page>
  );
}

export function CampaignsPage() {
  return (
    <Page title="Campaigns" description="One codebase-wide change, planned across every affected repository.">
      <Card>
        <EmptyState icon={<Workflow />} title="No campaigns">
          Creating a campaign requires discovered repositories, which are not available in this build.
        </EmptyState>
      </Card>
    </Page>
  );
}

export function AgentRunsPage() {
  return (
    <Page title="Agent Runs" description="Repository-level implementation sessions delegated to Devin.">
      <Card>
        <EmptyState icon={<Bot />} title="No agent runs">
          No Devin sessions have been started from this workstation. Delegation is not implemented in this build.
        </EmptyState>
      </Card>
    </Page>
  );
}

export function VerificationPage() {
  return (
    <Page title="Verification" description="Independent re-runs of each repository's checks against the agent's branch.">
      <Card>
        <EmptyState icon={<ShieldCheck />} title="No verification results">
          Verification runs after an agent produces a branch. No branches have been produced yet.
        </EmptyState>
      </Card>
    </Page>
  );
}
