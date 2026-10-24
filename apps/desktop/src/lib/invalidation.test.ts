import { describe, expect, it } from "vitest";
import { testClient } from "../test/render";
import { invalidateAfter, invalidationTargets } from "./invalidation";
import { queryKeys } from "./queryKeys";

function seed() {
  const client = testClient();
  const keys = {
    credentials: queryKeys.credentials,
    activityPage: queryKeys.activity.list({ limit: 25, offset: 0 }),
    database: queryKeys.health.database,
    keychain: queryKeys.health.keychain,
    settings: queryKeys.settings,
    active: queryKeys.processes.active,
  };
  for (const key of Object.values(keys)) client.setQueryData(key, { seeded: true });
  const stale = (key: readonly unknown[]) => client.getQueryState(key)?.isInvalidated ?? false;
  return { client, keys, stale };
}

describe("central invalidation", () => {
  it("credential changes refresh credentials, activity pages and database health only", async () => {
    const { client, keys, stale } = seed();
    await invalidateAfter(client, "credentials");
    expect(stale(keys.credentials)).toBe(true);
    expect(stale(keys.activityPage)).toBe(true);
    expect(stale(keys.database)).toBe(true);
    expect(stale(keys.settings)).toBe(false);
    expect(stale(keys.keychain)).toBe(false);
  });

  it("process events refresh the active set and activity", async () => {
    const { client, keys, stale } = seed();
    await invalidateAfter(client, "process");
    expect(stale(keys.active)).toBe(true);
    expect(stale(keys.activityPage)).toBe(true);
    expect(stale(keys.credentials)).toBe(false);
  });

  it("every change kind that writes activity invalidates the activity prefix", () => {
    for (const kind of ["onboarding", "credentials", "environment", "process"] as const) {
      expect(invalidationTargets(kind)).toContainEqual(queryKeys.activity.all);
    }
  });
});
