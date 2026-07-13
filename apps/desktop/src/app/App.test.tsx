import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { App } from "./App";
import { useAppStore } from "./store";

describe("DRPA Next desktop shell", () => {
  afterEach(cleanup);

  beforeEach(() => {
    useAppStore.setState({
      activeNavigation: "workbench",
      commandOpen: false,
      compactMode: false,
      inspectorOpen: true,
      selectedPackageId: "com.drpa.invoice-hub",
      selectedProfileId: "monthly",
      snapshot: null,
    });
  });

  it("loads the browser-preview workspace and renders the selected task", async () => {
    render(<App />);

    await waitFor(() => expect(screen.getByRole("heading", { name: "Monthly close", level: 1 })).toBeVisible());
    expect(screen.getByText("Verified package")).toBeVisible();
    expect(screen.getByRole("button", { name: /Run Monthly close/ })).toBeEnabled();
  });

  it("opens the command palette with the platform shortcut", async () => {
    render(<App />);
    fireEvent.keyDown(window, { key: "k", ctrlKey: true });

    expect(await screen.findByRole("dialog", { name: "Command palette" })).toBeVisible();
    expect(screen.getByPlaceholderText("Type a command or search packages…")).toHaveFocus();
  });

  it("navigates to the library without recreating host state", async () => {
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Library" }));

    expect(await screen.findByRole("heading", { name: "Library" })).toBeVisible();
    expect(screen.getByText("Invoice Hub")).toBeVisible();
  });
});
