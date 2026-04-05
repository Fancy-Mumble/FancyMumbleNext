/**
 * Unit tests for message deletion UI components:
 * - ConfirmDialog: reusable confirmation dialog
 * - MessageContextMenu: right-click menu on messages
 * - MessageSelectionBar: bulk selection action bar
 */

import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import ConfirmDialog from "../elements/ConfirmDialog";
import MessageSelectionBar from "../chat/MessageSelectionBar";
import MessageContextMenu, { type MessageContextMenuState } from "../chat/MessageContextMenu";
import type { ChatMessage } from "../../types";

// --- Helpers ------------------------------------------------------

function makeMessage(overrides: Partial<ChatMessage> = {}): ChatMessage {
  return {
    sender_session: 1,
    sender_name: "Alice",
    body: "Hello world",
    channel_id: 0,
    is_own: false,
    message_id: `msg-${Math.random().toString(36).slice(2)}`,
    timestamp: Date.now(),
    ...overrides,
  };
}

// --- ConfirmDialog tests ------------------------------------------

describe("ConfirmDialog", () => {
  it("renders title and body", () => {
    render(
      <ConfirmDialog
        title="Delete messages"
        body="Are you sure?"
        onConfirm={vi.fn()}
        onCancel={vi.fn()}
      />,
    );
    expect(screen.getByText("Delete messages")).toBeTruthy();
    expect(screen.getByText("Are you sure?")).toBeTruthy();
  });

  it("uses custom confirm/cancel labels", () => {
    render(
      <ConfirmDialog
        title="Test"
        body="Test body"
        confirmLabel="Yes, delete"
        cancelLabel="Nope"
        onConfirm={vi.fn()}
        onCancel={vi.fn()}
      />,
    );
    expect(screen.getByText("Yes, delete")).toBeTruthy();
    expect(screen.getByText("Nope")).toBeTruthy();
  });

  it("calls onConfirm when confirm button is clicked", () => {
    const onConfirm = vi.fn();
    render(
      <ConfirmDialog
        title="Test"
        body="Test body"
        confirmLabel="Delete"
        onConfirm={onConfirm}
        onCancel={vi.fn()}
      />,
    );
    fireEvent.click(screen.getByText("Delete"));
    expect(onConfirm).toHaveBeenCalledOnce();
  });

  it("calls onCancel when cancel button is clicked", () => {
    const onCancel = vi.fn();
    render(
      <ConfirmDialog
        title="Test"
        body="Test body"
        onConfirm={vi.fn()}
        onCancel={onCancel}
      />,
    );
    fireEvent.click(screen.getByText("Cancel"));
    expect(onCancel).toHaveBeenCalledOnce();
  });

  it("calls onCancel on Escape key", () => {
    const onCancel = vi.fn();
    render(
      <ConfirmDialog
        title="Test"
        body="Test body"
        onConfirm={vi.fn()}
        onCancel={onCancel}
      />,
    );
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onCancel).toHaveBeenCalledOnce();
  });
});

// --- MessageSelectionBar tests ------------------------------------

describe("MessageSelectionBar", () => {
  it("displays the selection count", () => {
    render(
      <MessageSelectionBar count={5} onDelete={vi.fn()} onCancel={vi.fn()} />,
    );
    expect(screen.getByText("5 selected")).toBeTruthy();
  });

  it("shows delete button with count", () => {
    render(
      <MessageSelectionBar count={3} onDelete={vi.fn()} onCancel={vi.fn()} />,
    );
    expect(screen.getByText("Delete (3)")).toBeTruthy();
  });

  it("disables delete button when count is 0", () => {
    render(
      <MessageSelectionBar count={0} onDelete={vi.fn()} onCancel={vi.fn()} />,
    );
    const deleteBtn = screen.getByText("Delete (0)").closest("button");
    expect(deleteBtn?.disabled).toBe(true);
  });

  it("calls onDelete when delete button is clicked", () => {
    const onDelete = vi.fn();
    render(
      <MessageSelectionBar count={2} onDelete={onDelete} onCancel={vi.fn()} />,
    );
    fireEvent.click(screen.getByText("Delete (2)"));
    expect(onDelete).toHaveBeenCalledOnce();
  });

  it("calls onCancel when cancel button is clicked", () => {
    const onCancel = vi.fn();
    render(
      <MessageSelectionBar count={2} onDelete={vi.fn()} onCancel={onCancel} />,
    );
    fireEvent.click(screen.getByText("Cancel"));
    expect(onCancel).toHaveBeenCalledOnce();
  });
});

// --- MessageContextMenu tests -------------------------------------

describe("MessageContextMenu", () => {
  const defaultMsg = makeMessage();

  function renderMenu(
    opts: {
      canDelete?: boolean;
      onClose?: () => void;
      onDelete?: (msg: ChatMessage) => void;
      onSelectMode?: (msg: ChatMessage) => void;
    } = {},
  ) {
    const menu: MessageContextMenuState = {
      x: 100,
      y: 200,
      message: defaultMsg,
    };
    return render(
      <MessageContextMenu
        menu={menu}
        canDelete={opts.canDelete ?? true}
        onClose={opts.onClose ?? vi.fn()}
        onDelete={opts.onDelete ?? vi.fn()}
        onSelectMode={opts.onSelectMode ?? vi.fn()}
      />,
    );
  }

  it("shows Delete message when canDelete is true", () => {
    renderMenu({ canDelete: true });
    expect(screen.getByText("Delete message")).toBeTruthy();
  });

  it("hides Delete message when canDelete is false", () => {
    renderMenu({ canDelete: false });
    expect(screen.queryByText("Delete message")).toBeNull();
  });

  it("hides Select messages when canDelete is false", () => {
    renderMenu({ canDelete: false });
    expect(screen.queryByText("Select messages")).toBeNull();
  });

  it("calls onDelete when Delete message is clicked", () => {
    const onDelete = vi.fn();
    const onClose = vi.fn();
    renderMenu({ onDelete, onClose });
    fireEvent.click(screen.getByText("Delete message"));
    expect(onDelete).toHaveBeenCalledWith(defaultMsg);
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("calls onSelectMode when Select messages is clicked", () => {
    const onSelectMode = vi.fn();
    const onClose = vi.fn();
    renderMenu({ canDelete: true, onSelectMode, onClose });
    fireEvent.click(screen.getByText("Select messages"));
    expect(onSelectMode).toHaveBeenCalledWith(defaultMsg);
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("calls onClose on Escape key", () => {
    const onClose = vi.fn();
    renderMenu({ onClose });
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalledOnce();
  });
});
