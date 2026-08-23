import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

const { ImportStatus } = await import("./ImportStatus");
const { useImportsStore } = await import("../stores/imports");

function showImporting() {
  useImportsStore.setState({
    active: {
      bookId: "book-1",
      title: "A Big Textbook",
      stage: "fetching",
      current: 3,
      total: 40,
    },
    completed: null,
    error: null,
  });
  render(<ImportStatus onOpen={() => {}} />);
}

afterEach(() => {
  useImportsStore.setState({ active: null, completed: null, error: null });
  vi.restoreAllMocks();
});

describe("cancelling from the progress strip", () => {
  it("offers a cancel control while an import is running", () => {
    // The strip is the only place an import is visible at all, so it is the
    // only place the reader could ask for it to stop.
    showImporting();

    expect(
      screen.getByRole("button", { name: /cancel importing A Big Textbook/i }),
    ).toBeInTheDocument();
  });

  it("cancels the import when it is pressed", async () => {
    const cancel = vi.fn(async () => undefined);
    useImportsStore.setState({ cancel });
    showImporting();

    await userEvent.click(
      screen.getByRole("button", { name: /cancel importing/i }),
    );

    expect(cancel).toHaveBeenCalled();
  });

  it("offers nothing to cancel once the import has finished", () => {
    // The finished strip already has a Dismiss X. A second control that reads
    // as "undo the import" next to "Open" would be two different verbs sharing
    // one glyph.
    useImportsStore.setState({
      active: null,
      completed: { documentId: "doc-1", title: "A Big Textbook" },
      error: null,
    });
    render(<ImportStatus onOpen={() => {}} />);

    expect(
      screen.queryByRole("button", { name: /cancel importing/i }),
    ).not.toBeInTheDocument();
  });
});
