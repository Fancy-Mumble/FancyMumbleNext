import { useCallback, useEffect, useState, type FormEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useAppStore } from "../store";
import {
  getSavedServers,
  addServer,
  removeServer,
} from "../serverStorage";
import { getPreferences } from "../preferencesStorage";
import type { SavedServer, ServerPingResult, UserMode } from "../types";
import ServerList from "../components/ServerList";
import PublicServerList from "../components/PublicServerList";
import PasswordDialog from "../components/PasswordDialog";
import styles from "./ConnectPage.module.css";

type View = "loading" | "servers" | "wizard" | "public";

/** Module-level cache: "host:port" -> epoch-ms of last ping invocation. */
const pingCache = new Map<string, number>();

interface StepDef {
  readonly title: string;
  readonly subtitle: string;
  readonly hint: string;
}

/** Wizard steps for **expert** mode - full control. */
const STEPS_EXPERT: StepDef[] = [
  {
    title: "Server address",
    subtitle: "Where is your Mumble server?",
    hint: "Enter the hostname or IP address your server admin gave you. The default port is 64738. Select a client certificate for TLS auth, or leave it as 'None' for anonymous connections.",
  },
  {
    title: "Your identity",
    subtitle: "How should others see you?",
    hint: "Pick a username that will be shown to other users on the server. You can change it later in most servers.",
  },
  {
    title: "Give it a name",
    subtitle: "Almost there!",
    hint: "Choose a friendly label so you can recognise this server later. Leave it blank to use the server address.",
  },
];

/** Wizard steps for **normal** mode - streamlined, no port or label. */
const STEPS_NORMAL: StepDef[] = [
  {
    title: "Server address",
    subtitle: "Where is your Mumble server?",
    hint: "Enter the server address your admin gave you - we'll take care of the rest.",
  },
  {
    title: "Your identity",
    subtitle: "How should others see you?",
    hint: "Pick a username that will be shown to other users on the server.",
  },
];

export default function ConnectPage() {
  const { connect, disconnect, status, error, passwordRequired, pendingConnect, retryWithPassword, dismissPasswordPrompt } = useAppStore();
  const isConnecting = status === "connecting";

  /* -- which server card is actively connecting ------------------- */
  const [connectingServerId, setConnectingServerId] = useState<string | null>(null);

  // Clear the connecting indicator when we leave the "connecting" state
  useEffect(() => {
    if (!isConnecting) setConnectingServerId(null);
  }, [isConnecting]);

  /* -- user mode ------------------------------------------------- */
  const [userMode, setUserMode] = useState<UserMode>("normal");
  const [defaultUsername, setDefaultUsername] = useState("");
  const STEPS = userMode === "normal" ? STEPS_NORMAL : STEPS_EXPERT;

  /* -- saved servers --------------------------------------------- */
  const [savedServers, setSavedServers] = useState<SavedServer[]>([]);
  const [view, setView] = useState<View>("loading");

  /* -- ping results keyed by server id --------------------------- */
  const [pings, setPings] = useState<Record<string, ServerPingResult>>({});

  /**
   * Ping a list of servers, throttled to at most once per 60 s per
   * host:port.  This prevents the Mumble server from seeing a flood of
   * raw TCP connections (which it logs as ban rejections for banned IPs).
   */
  const pingServers = useCallback((servers: SavedServer[]) => {
    const THROTTLE_MS = 60_000;
    const now = Date.now();

    for (const s of servers) {
      const key = `${s.host}:${s.port}`;
      const last = pingCache.get(key);
      if (last !== undefined && now - last < THROTTLE_MS) {
        // Still fresh - re-use cached result already in state.
        continue;
      }
      pingCache.set(key, now);

      invoke<ServerPingResult>("ping_server", { host: s.host, port: s.port })
        .then((result) =>
          setPings((prev) => ({ ...prev, [s.id]: result })),
        )
        .catch(() =>
          setPings((prev) => ({
            ...prev,
            [s.id]: { online: false, latency_ms: null, user_count: null, max_user_count: null },
          })),
        );
    }
  }, []);

  useEffect(() => {
    Promise.all([getSavedServers(), getPreferences()]).then(([list, prefs]) => {
      setUserMode(prefs.userMode);
      setDefaultUsername(prefs.defaultUsername);
      setUsername(prefs.defaultUsername);
      setSavedServers(list);
      setView(list.length > 0 ? "servers" : "wizard");
      if (list.length > 0) pingServers(list);
    });
  }, [pingServers]);

  /* -- certificate state ----------------------------------------- */
  const [availableCerts, setAvailableCerts] = useState<string[]>([]);
  const [certLabel, setCertLabel] = useState<string>("default");
  const [creatingCert, setCreatingCert] = useState(false);
  const [newCertName, setNewCertName] = useState("");

  const refreshCerts = () =>
    invoke<string[]>("list_certificates")
      .then(setAvailableCerts)
      .catch(() => setAvailableCerts([]));

  useEffect(() => {
    refreshCerts();
  }, []);

  const handleCreateCert = async () => {
    const name = newCertName.trim();
    if (!name) return;
    await invoke("generate_certificate", { label: name });
    await refreshCerts();
    setCertLabel(name);
    setNewCertName("");
    setCreatingCert(false);
  };

  /* -- wizard state ---------------------------------------------- */
  const [step, setStep] = useState(0);
  const [label, setLabel] = useState("");
  const [host, setHost] = useState("");
  const [port, setPort] = useState("64738");
  const [username, setUsername] = useState("");
  /** Normal-mode only: whether the user is using their stored default name. */
  const [usingDefaultName, setUsingDefaultName] = useState(true);

  const resetWizard = () => {
    setStep(0);
    setLabel("");
    setHost("");
    setPort("64738");
    setUsername(defaultUsername);
    setUsingDefaultName(true);
    setCertLabel("default");
    setCreatingCert(false);
    setNewCertName("");
  };

  /* -- per-step validation --------------------------------------- */
  const canAdvance = (): boolean => {
    if (step === 0) return host.trim().length > 0;
    if (step === 1) {
      // Happy path: using the stored default name is always valid.
      if (userMode === "normal" && defaultUsername && usingDefaultName) return true;
      return username.trim().length > 0;
    }
    return true; // label is optional
  };

  /* -- actions --------------------------------------------------- */
  const handleNext = (e: FormEvent) => {
    e.preventDefault();
    if (!canAdvance()) return;
    if (step < STEPS.length - 1) {
      setStep((s) => s + 1);
    }
  };

  const handleBack = () => {
    if (step > 0) setStep((s) => s - 1);
  };

  const handleConnectAndSave = async () => {
    if (!host || !username) return;
    const p = Number.parseInt(port) || 64738;
    const resolvedCert = certLabel || null;
    const entry = await addServer({
      label: label || host,
      host,
      port: p,
      username,
      cert_label: resolvedCert,
    });
    setSavedServers((prev) => [entry, ...prev]);
    await connect(host, p, username, resolvedCert);
  };

  const handleQuickConnectForm = async () => {
    if (!host || !username) return;
    const p = Number.parseInt(port) || 64738;
    await connect(host, p, username, certLabel || null);
  };

  const handleQuickConnect = async (server: SavedServer) => {
    setConnectingServerId(server.id);
    await connect(server.host, server.port, server.username, server.cert_label);
  };

  const handleCancelConnect = useCallback(async () => {
    await disconnect();
    setConnectingServerId(null);
  }, [disconnect]);

  const handleDelete = async (id: string) => {
    await removeServer(id);
    setSavedServers((prev) => {
      const next = prev.filter((s) => s.id !== id);
      if (next.length === 0) setView("wizard");
      return next;
    });
  };

  const handleShowWizard = () => {
    resetWizard();
    setView("wizard");
  };

  const handleShowPublic = () => {
    setView("public");
  };

  const handlePublicConnect = (pubHost: string, pubPort: number) => {
    // Pre-fill the wizard with the public server's address
    resetWizard();
    setHost(pubHost);
    setPort(String(pubPort));
    setStep(1); // skip to username step
    setView("wizard");
  };

  const handleBackToServers = () => {
    setView(savedServers.length > 0 ? "servers" : "wizard");
    resetWizard();
  };

  /* -- render helpers -------------------------------------------- */
  const isLastStep = step === STEPS.length - 1;
  const currentStep = STEPS[step];

  if (view === "loading") return null;

  const cardClass = [styles.card, view === "public" && styles.cardWide].filter(Boolean).join(" ");

  return (
    <div className={styles.page}>
      <div className={cardClass}>
        {/* Logo - always visible */}
        <div className={styles.logo}>
          <div className={styles.logoIcon}>M</div>
          <h1 className={styles.title}>Fancy Mumble</h1>
          <p className={styles.subtitle}>
            {view === "servers" || view === "public"
              ? "Choose a server to connect"
              : currentStep.subtitle}
          </p>
        </div>

        {/* Error banner */}
        {error && (
          <div className={styles.error}>
            <span className={styles.errorIcon}>!</span>
            {error}
          </div>
        )}

        {/* -------- Server list view (happy path) -------- */}
        {view === "servers" && (
          <>
            <ServerList
              servers={savedServers}
              pings={pings}
              onConnect={handleQuickConnect}
              onDelete={handleDelete}
              onAddNew={handleShowWizard}
              onCancelConnect={handleCancelConnect}
              disabled={isConnecting}
              connectingId={connectingServerId}
            />
            <button
              className={styles.publicLink}
              onClick={handleShowPublic}
              disabled={isConnecting}
              type="button"
            >
              Browse public servers
            </button>
          </>
        )}

        {/* -------- Public server list ------------------- */}
        {view === "public" && (
          <PublicServerList
            onConnect={handlePublicConnect}
            onBack={handleBackToServers}
            disabled={isConnecting}
          />
        )}

        {/* -------- Multi-step wizard -------------------- */}
        {view === "wizard" && (
          <>
            {/* Back navigation */}
            {(savedServers.length > 0 || step > 0) && (
              <button
                className={styles.backLink}
                onClick={step > 0 ? handleBack : handleBackToServers}
                disabled={isConnecting}
                type="button"
              >
                ← {step > 0 ? "Back" : "Saved servers"}
              </button>
            )}

            {/* Step indicator */}
            <div className={styles.stepIndicator}>
              {STEPS.map((_, i) => (
                <div
                  key={`step-${STEPS[i].title}`}
                  className={`${styles.stepDot} ${i <= step ? styles.stepDotActive : ""} ${i === step ? styles.stepDotCurrent : ""}`}
                />
              ))}
              <span className={styles.stepLabel}>
                Step {step + 1} of {STEPS.length}
              </span>
            </div>

            <form
              onSubmit={isLastStep ? (e) => { e.preventDefault(); handleConnectAndSave(); } : handleNext}
              className={styles.form}
            >
              {/* -- Step 0: Server address ---------------- */}
              {step === 0 && (
                <>
                  <div className={styles.field}>
                    <label className={styles.label}>Server address</label>
                    <input
                      className={styles.input}
                      type="text"
                      placeholder="mumble.example.com"
                      value={host}
                      onChange={(e) => setHost(e.target.value)}
                      disabled={isConnecting}
                      autoFocus
                    />
                  </div>
                  {userMode !== "normal" && (
                    <>
                      <div className={styles.field}>
                        <label className={styles.label}>Port</label>
                        <input
                          className={styles.input}
                          type="text"
                          placeholder="64738"
                          value={port}
                          onChange={(e) => setPort(e.target.value)}
                          disabled={isConnecting}
                        />
                      </div>
                      <div className={styles.field}>
                        <label className={styles.label}>Client certificate</label>
                        <select
                          className={styles.input}
                          value={creatingCert ? "__new__" : certLabel}
                          onChange={(e) => {
                            if (e.target.value === "__new__") {
                              setCreatingCert(true);
                            } else {
                              setCreatingCert(false);
                              setCertLabel(e.target.value);
                            }
                          }}
                          disabled={isConnecting}
                        >
                          <option value="">None (anonymous)</option>
                          {availableCerts.map((c) => (
                            <option key={c} value={c}>
                              {c === "default" ? `${c} (auto-generated)` : c}
                            </option>
                          ))}
                          <option value="__new__">+ Create new identity…</option>
                        </select>
                        {creatingCert && (
                          <div className={styles.newCertRow}>
                            <input
                              className={styles.input}
                              type="text"
                              placeholder="Identity name"
                              value={newCertName}
                              onChange={(e) => setNewCertName(e.target.value)}
                              onKeyDown={(e) => {
                                if (e.key === "Enter") {
                                  e.preventDefault();
                                  handleCreateCert();
                                }
                              }}
                              autoFocus
                            />
                            <button
                              type="button"
                              className={styles.buttonGhost}
                              onClick={handleCreateCert}
                              disabled={!newCertName.trim()}
                            >
                              Create
                            </button>
                            <button
                              type="button"
                              className={styles.buttonGhost}
                              onClick={() => {
                                setCreatingCert(false);
                                setNewCertName("");
                              }}
                            >
                              Cancel
                            </button>
                          </div>
                        )}
                      </div>
                    </>
                  )}
                </>
              )}

              {/* -- Step 1: Username ---------------------- */}
              {step === 1 && (
                <>
                  {/* Normal mode + stored default: happy path confirmation */}
                  {userMode === "normal" && defaultUsername && usingDefaultName ? (
                    <div className={styles.usernameSummary}>
                      <div className={styles.usernameConfirm}>
                        <span className={styles.usernameCheckmark}>✓</span>
                        <span className={styles.usernameValue}>{defaultUsername}</span>
                      </div>
                      <button
                        type="button"
                        className={styles.usernameOtherLink}
                        onClick={() => {
                          setUsingDefaultName(false);
                          setUsername(defaultUsername);
                        }}
                        disabled={isConnecting}
                      >
                        Use a different name for this server
                      </button>
                    </div>
                  ) : (
                    <div className={styles.field}>
                      <label className={styles.label}>Username</label>
                      <input
                        className={styles.input}
                        type="text"
                        placeholder="Your name"
                        value={username}
                        onChange={(e) => setUsername(e.target.value)}
                        disabled={isConnecting}
                        autoFocus
                      />
                      {userMode === "normal" && defaultUsername && !usingDefaultName && (
                        <button
                          type="button"
                          className={styles.usernameOtherLink}
                          onClick={() => {
                            setUsingDefaultName(true);
                            setUsername(defaultUsername);
                          }}
                          disabled={isConnecting}
                        >
                          ← Use my default name ({defaultUsername})
                        </button>
                      )}
                    </div>
                  )}
                </>
              )}

              {/* -- Step 2: Label (expert only) ---------- */}
              {step === 2 && userMode !== "normal" && (
                <div className={styles.field}>
                  <label className={styles.label}>Server label (optional)</label>
                  <input
                    className={styles.input}
                    type="text"
                    placeholder={host || "My Mumble Server"}
                    value={label}
                    onChange={(e) => setLabel(e.target.value)}
                    disabled={isConnecting}
                    autoFocus
                  />
                </div>
              )}

              {/* Hint card */}
              <div className={styles.hint}>
                <span className={styles.hintIcon}>💡</span>
                <span>{currentStep.hint}</span>
              </div>

              {/* Action buttons */}
              {isLastStep ? (
                <div className={styles.buttonRow}>
                  <button
                    className={styles.buttonGhost}
                    type="button"
                    onClick={handleQuickConnectForm}
                    disabled={isConnecting || !host || !username}
                  >
                    Quick Connect
                  </button>
                  <button
                    className={styles.button}
                    type="submit"
                    disabled={isConnecting || !host || !username}
                  >
                    {isConnecting ? (
                      <>
                        <span className={styles.spinner} />
                        Connecting...
                      </>
                    ) : (
                      "Connect & Save"
                    )}
                  </button>
                </div>
              ) : (
                <button
                  className={styles.button}
                  type="submit"
                  disabled={!canAdvance()}
                >
                  Continue
                </button>
              )}
            </form>
          </>
        )}
      </div>

      <PasswordDialog
        open={passwordRequired}
        onSubmit={retryWithPassword}
        onCancel={dismissPasswordPrompt}
        serverHost={pendingConnect?.host}
        username={pendingConnect?.username}
        error={error}
      />
    </div>
  );
}
