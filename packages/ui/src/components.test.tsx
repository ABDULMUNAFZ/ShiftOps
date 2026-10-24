// @vitest-environment jsdom
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { Button, ErrorState } from "./index";

afterEach(cleanup);

describe("Button", () => {
  it("defaults to type=button so it never submits forms accidentally", () => {
    render(<Button>Run</Button>);
    expect(screen.getByRole("button", { name: "Run" }).getAttribute("type")).toBe("button");
  });

  it("merges conflicting classes in favour of the caller", () => {
    render(<Button className="h-12">Tall</Button>);
    const cls = screen.getByRole("button", { name: "Tall" }).className;
    expect(cls).toContain("h-12");
    expect(cls).not.toContain("h-8");
  });
});

describe("ErrorState", () => {
  it("renders the underlying error message verbatim", () => {
    render(<ErrorState title="Check failed" error={new Error("spawn git ENOENT")} />);
    expect(screen.getByRole("alert").textContent).toContain("spawn git ENOENT");
  });
});
