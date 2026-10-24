import { fireEvent, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { renderWithClient } from "../test/render";

const native = vi.hoisted(() => ({
  listCampaigns: vi.fn(),
  listRepositories: vi.fn(),
  createCampaign: vi.fn(),
}));
vi.mock("../lib/native", () => ({ native }));

const { CampaignsPage } = await import("./CampaignsPage");

beforeEach(() => {
  for (const fn of Object.values(native)) fn.mockReset();
  native.listCampaigns.mockResolvedValue({ items: [], total: 0, limit: 50, offset: 0 });
  native.listRepositories.mockResolvedValue({
    items: [{ id: "r1", name: "orders-api", localPath: "/work/orders-api", remoteUrl: null, trustStatus: "trusted" }],
    total: 1,
    limit: 200,
    offset: 0,
  });
  native.createCampaign.mockResolvedValue({ id: "c1" });
});

function renderPage() {
  return renderWithClient(
    <MemoryRouter>
      <CampaignsPage />
    </MemoryRouter>,
  );
}

describe("CampaignsPage", () => {
  it("shows an honest empty state", async () => {
    renderPage();
    expect(await screen.findByText("No campaigns")).toBeTruthy();
  });

  it("creates a campaign from selected real repositories with the goal", async () => {
    renderPage();
    await screen.findByText("No campaigns");
    fireEvent.click(screen.getByRole("button", { name: /New campaign/ }));
    fireEvent.change(screen.getByPlaceholderText("Node 22 rollout"), { target: { value: "Node 22" } });
    fireEvent.click(screen.getByRole("checkbox"));
    fireEvent.click(screen.getByRole("button", { name: "Create campaign" }));
    await waitFor(() => expect(native.createCampaign).toHaveBeenCalled());
    const [definition, ids] = native.createCampaign.mock.calls[0] as [Record<string, unknown>, string[]];
    expect(ids).toEqual(["r1"]);
    expect(definition.objective).toBe("Upgrade Node.js 18 to Node.js 22");
    expect(definition.targetCondition).toEqual({ runtime: "node", major: "22" });
  });
});
