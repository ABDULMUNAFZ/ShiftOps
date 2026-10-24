import { describe, expect, it } from "vitest";
import { isGitHubHost, parseGitRemote } from "./index";

describe("parseGitRemote", () => {
  it.each([
    ["https://github.com/acme/api.git", { host: "github.com", owner: "acme", name: "api" }],
    ["https://github.com/acme/api", { host: "github.com", owner: "acme", name: "api" }],
    ["git@github.com:acme/web-app.git", { host: "github.com", owner: "acme", name: "web-app" }],
    ["ssh://git@github.com/acme/sdk.js.git", { host: "github.com", owner: "acme", name: "sdk.js" }],
  ])("parses %s", (remote, expected) => {
    expect(parseGitRemote(remote)).toEqual(expected);
  });

  it.each(["", "not a url", "https://github.com/acme", "https://gitlab.com/group/sub/repo", "file:///tmp/repo"])(
    "rejects %s",
    (remote) => {
      expect(parseGitRemote(remote)).toBeNull();
    },
  );
});

describe("isGitHubHost", () => {
  it("accepts github.com and GHE.com tenants only", () => {
    expect(isGitHubHost({ host: "github.com", owner: "a", name: "b" })).toBe(true);
    expect(isGitHubHost({ host: "acme.ghe.com", owner: "a", name: "b" })).toBe(true);
    expect(isGitHubHost({ host: "gitlab.com", owner: "a", name: "b" })).toBe(false);
  });
});
