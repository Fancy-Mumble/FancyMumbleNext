/**
 * Unit tests for PublicServerList component.
 *
 * Covers: consent gate, server rendering, sorting, fuzzy search,
 * error display, and connect callback.
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";

// --- Mocks --------------------------------------------------------

const invokeMock = vi.fn<(cmd: string, args?: unknown) => Promise<unknown>>();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(args[0] as string, args[1]),
}));

// Import after mocks so the module picks up the mocked invoke.
import PublicServerList from "../PublicServerList";
import { clearPingCache } from "../PublicServerList";
import type { PublicServer } from "../../types";

// --- Helpers ------------------------------------------------------

function makeServer(overrides: Partial<PublicServer> = {}): PublicServer {
  return {
    name: "Test Server",
    country: "Germany",
    country_code: "DE",
    ip: "10.0.0.1",
    port: 64738,
    region: "Bavaria",
    url: "https://example.com",
    ...overrides,
  };
}

const SAMPLE_SERVERS: PublicServer[] = [
  makeServer({ name: "Alpha", country: "Canada", country_code: "CA", ip: "1.1.1.1" }),
  makeServer({ name: "Beta", country: "Japan", country_code: "JP", ip: "2.2.2.2" }),
  makeServer({ name: "Gamma", country: "France", country_code: "FR", ip: "3.3.3.3" }),
];

function renderList(props: Partial<React.ComponentProps<typeof PublicServerList>> = {}) {
  const defaults = {
    onConnect: vi.fn(),
    onBack: vi.fn(),
    disabled: false,
  };
  return render(<PublicServerList {...defaults} {...props} />);
}

/** Click the consent button and wait for fetch to resolve. */
async function consentAndWait() {
  fireEvent.click(screen.getByText("I understand, show servers"));
  await waitFor(() => {
    expect(invokeMock).toHaveBeenCalledWith("fetch_public_servers", undefined);
  });
}

// --- Tests --------------------------------------------------------

beforeEach(() => {
  vi.clearAllMocks();
  clearPingCache();
  // Default: fetch returns sample servers, ping returns per-server user counts
  const pingData: Record<string, { online: boolean; latency_ms: number; user_count: number; max_user_count: number }> = {
    "1.1.1.1": { online: true, latency_ms: 42, user_count: 5, max_user_count: 50 },
    "2.2.2.2": { online: true, latency_ms: 80, user_count: 12, max_user_count: 100 },
    "3.3.3.3": { online: true, latency_ms: 20, user_count: 3, max_user_count: 30 },
  };
  invokeMock.mockImplementation((cmd: string, args?: unknown) => {
    if (cmd === "fetch_public_servers") return Promise.resolve(SAMPLE_SERVERS);
    if (cmd === "ping_server") {
      const { host } = (args ?? {}) as { host?: string };
      const data = host ? pingData[host] : undefined;
      return Promise.resolve(data ?? { online: true, latency_ms: 42, user_count: null, max_user_count: null });
    }
    return Promise.reject(new Error(`Unknown command: ${cmd}`));
  });
});

describe("Consent gate", () => {
  it("shows consent text before loading servers", () => {
    renderList();
    expect(screen.getByText("I understand, show servers")).toBeTruthy();
    expect(screen.queryByPlaceholderText("Search servers...")).toBeNull();
  });

  it("does not call fetch before consent", () => {
    renderList();
    expect(invokeMock).not.toHaveBeenCalledWith("fetch_public_servers", undefined);
  });

  it("fetches servers after consent", async () => {
    renderList();
    await consentAndWait();
    expect(invokeMock).toHaveBeenCalledWith("fetch_public_servers", undefined);
  });

  it("shows back button on consent screen", () => {
    const onBack = vi.fn();
    renderList({ onBack });
    fireEvent.click(screen.getByText("Saved servers"));
    expect(onBack).toHaveBeenCalledOnce();
  });
});

describe("Server rendering", () => {
  it("displays server names after consent", async () => {
    renderList();
    await consentAndWait();
    await waitFor(() => {
      expect(screen.getByText("Alpha")).toBeTruthy();
      expect(screen.getByText("Beta")).toBeTruthy();
      expect(screen.getByText("Gamma")).toBeTruthy();
    });
  });

  it("displays country names", async () => {
    renderList();
    await consentAndWait();
    await waitFor(() => {
      expect(screen.getByText("Canada")).toBeTruthy();
      expect(screen.getByText("Japan")).toBeTruthy();
      expect(screen.getByText("France")).toBeTruthy();
    });
  });

  it("displays user counts from ping data", async () => {
    renderList();
    await consentAndWait();
    await waitFor(() => {
      // User counts: Alpha=5/50, Beta=12/100, Gamma=3/30
      expect(screen.getByText("5/50")).toBeTruthy();
      expect(screen.getByText("12/100")).toBeTruthy();
      expect(screen.getByText("3/30")).toBeTruthy();
    });
  });

  it("shows empty state when no servers returned", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "fetch_public_servers") return Promise.resolve([]);
      return Promise.resolve(null);
    });
    renderList();
    await consentAndWait();
    await waitFor(() => {
      expect(screen.getByText("No public servers found.")).toBeTruthy();
    });
  });
});

describe("Error handling", () => {
  it("displays error when fetch fails", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "fetch_public_servers") return Promise.reject("Server returned HTTP 501 Not Implemented");
      return Promise.resolve(null);
    });
    renderList();
    await consentAndWait();
    await waitFor(() => {
      expect(screen.getByText(/Server returned HTTP 501/)).toBeTruthy();
    });
  });

  it("displays error on network failure", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "fetch_public_servers") return Promise.reject("Failed to fetch public server list: connection refused");
      return Promise.resolve(null);
    });
    renderList();
    await consentAndWait();
    await waitFor(() => {
      expect(screen.getByText(/Failed to fetch/)).toBeTruthy();
    });
  });
});

describe("Sorting", () => {
  it("sorts by server name ascending by default", async () => {
    renderList();
    await consentAndWait();
    await waitFor(() => screen.getByText("Alpha"));

    const rows = screen.getAllByRole("row").slice(1); // skip header row
    expect(rows[0].textContent).toContain("Alpha");
    expect(rows[1].textContent).toContain("Beta");
    expect(rows[2].textContent).toContain("Gamma");
  });

  it("reverses sort direction on second click", async () => {
    renderList();
    await consentAndWait();
    await waitFor(() => screen.getByText("Alpha"));

    // Click "Server" header twice for descending
    fireEvent.click(screen.getByText("Server"));
    const rows = screen.getAllByRole("row").slice(1);
    expect(rows[0].textContent).toContain("Gamma");
    expect(rows[2].textContent).toContain("Alpha");
  });

  it("sorts by country when clicking Country header", async () => {
    renderList();
    await consentAndWait();
    await waitFor(() => screen.getByText("Alpha"));

    fireEvent.click(screen.getByText("Country"));
    const rows = screen.getAllByRole("row").slice(1);
    // Canada, France, Japan (alphabetical)
    expect(rows[0].textContent).toContain("Canada");
    expect(rows[1].textContent).toContain("France");
    expect(rows[2].textContent).toContain("Japan");
  });

  it("sorts by user count when clicking Users header", async () => {
    renderList();
    await consentAndWait();
    // Wait for ping data to arrive
    await waitFor(() => screen.getByText("5/50"));

    fireEvent.click(screen.getByText("Users"));
    const rows = screen.getAllByRole("row").slice(1);
    // 3, 5, 12 (ascending by user_count from ping)
    expect(rows[0].textContent).toContain("Gamma");
    expect(rows[1].textContent).toContain("Alpha");
    expect(rows[2].textContent).toContain("Beta");
  });
});

describe("Fuzzy search", () => {
  it("filters servers by name", async () => {
    renderList();
    await consentAndWait();
    await waitFor(() => screen.getByText("Alpha"));

    fireEvent.change(screen.getByPlaceholderText("Search servers..."), {
      target: { value: "bet" },
    });

    expect(screen.getByText("Beta")).toBeTruthy();
    expect(screen.queryByText("Alpha")).toBeNull();
    expect(screen.queryByText("Gamma")).toBeNull();
  });

  it("filters servers by country", async () => {
    renderList();
    await consentAndWait();
    await waitFor(() => screen.getByText("Alpha"));

    fireEvent.change(screen.getByPlaceholderText("Search servers..."), {
      target: { value: "japan" },
    });

    expect(screen.getByText("Beta")).toBeTruthy();
    expect(screen.queryByText("Alpha")).toBeNull();
  });

  it("shows no-match message for impossible query", async () => {
    renderList();
    await consentAndWait();
    await waitFor(() => screen.getByText("Alpha"));

    fireEvent.change(screen.getByPlaceholderText("Search servers..."), {
      target: { value: "zzzzzzzzz" },
    });

    expect(screen.getByText("No servers match your search.")).toBeTruthy();
  });
});

describe("Connect callback", () => {
  it("calls onConnect with host and port when row is clicked", async () => {
    const onConnect = vi.fn();
    renderList({ onConnect });
    await consentAndWait();
    await waitFor(() => screen.getByText("Alpha"));

    fireEvent.click(screen.getByText("Alpha"));
    expect(onConnect).toHaveBeenCalledWith("1.1.1.1", 64738);
  });

  it("does not call onConnect when disabled", async () => {
    const onConnect = vi.fn();
    renderList({ onConnect, disabled: true });
    await consentAndWait();
    await waitFor(() => screen.getByText("Alpha"));

    fireEvent.click(screen.getByText("Alpha"));
    expect(onConnect).not.toHaveBeenCalled();
  });
});
