import { MenuIcon } from "../icons";
import { invoke } from "@tauri-apps/api/core";
import { lazy, Suspense, useEffect, useState, useCallback, useRef } from "react";
import { useNavigate } from "react-router-dom";
import { useAppStore } from "../store";
import { isMobile } from "../utils/platform";
import { useSwipeDrawer } from "../hooks/useSwipeDrawer";
import { usePasswordPrompt } from "../hooks/usePasswordPrompt";
import ChannelSidebar from "../components/sidebar/ChannelSidebar";
import ChatView from "../components/chat/ChatView";
import PasswordDialog from "../components/server/PasswordDialog";
import styles from "./ChatPage.module.css";

const ServerInfoPanel = lazy(() => import("../components/server/ServerInfoPanel"));
const ChannelInfoPanel = lazy(() => import("../components/sidebar/ChannelInfoPanel"));
const UserProfileView = lazy(() => import("../components/user/UserProfileView"));
const MobileProfileSheet = lazy(() => import("../components/user/MobileProfileSheet"));
const MobileBottomSheet = lazy(() => import("../components/elements/MobileBottomSheet"));

export default function ChatPage() {
  const status = useAppStore((s) => s.status);
  const selectedChannel = useAppStore((s) => s.selectedChannel);
  const selectedUser = useAppStore((s) => s.selectedUser);
  const selectedDmUser = useAppStore((s) => s.selectedDmUser);
  const sessions = useAppStore((s) => s.sessions);
  const activeServerId = useAppStore((s) => s.activeServerId);
  const error = useAppStore((s) => s.error);
  const passwordRequired = useAppStore((s) => s.passwordRequired);
  const pendingConnect = useAppStore((s) => s.pendingConnect);
  const dismissPasswordPrompt = useAppStore((s) => s.dismissPasswordPrompt);
  const connect = useAppStore((s) => s.connect);
  const refreshSessions = useAppStore((s) => s.refreshSessions);
  const navigate = useNavigate();

  const [isReconnecting, setIsReconnecting] = useState(false);

  const { handleSubmit: handlePasswordSubmit, handleChangeUsername, showSaveOption } =
    usePasswordPrompt();


  // On desktop, track whether the viewport is narrow (<= 768px).
  // When narrow, the sidebar uses the same slide-out drawer as mobile.
  const [isNarrow, setIsNarrow] = useState(
    () => !isMobile && window.matchMedia("(max-width: 768px)").matches,
  );

  useEffect(() => {
    if (isMobile) return;
    const mql = window.matchMedia("(max-width: 768px)");
    const handler = (e: MediaQueryListEvent) => setIsNarrow(e.matches);
    mql.addEventListener("change", handler);
    return () => mql.removeEventListener("change", handler);
  }, [isMobile]);

  const useDrawer = isMobile || isNarrow;
  const [sidebarOpen, setSidebarOpen] = useState(!useDrawer);
  const pageRef = useRef<HTMLDivElement>(null);
  const drawerRef = useRef<HTMLDivElement>(null);

  // Auto-open sidebar when leaving narrow mode, auto-close when entering it.
  useEffect(() => {
    setSidebarOpen(!useDrawer);
  }, [useDrawer]);

  const [showServerInfo, setShowServerInfo] = useState(false);
  const [showChannelInfo, setShowChannelInfo] = useState(false);
  const [searchChannelId, setSearchChannelId] = useState<number | null>(null);

  const toggleSidebar = useCallback(() => setSidebarOpen((v) => !v), []);
  const toggleServerInfo = useCallback(() => {
    setShowServerInfo((v) => !v);
    setShowChannelInfo(false);
  }, []);
  const toggleChannelInfo = useCallback(() => {
    setShowChannelInfo((v) => !v);
    setShowServerInfo(false);
  }, []);
  const openSidebar = useCallback(() => setSidebarOpen(true), []);
  const closeSidebar = useCallback(() => {
    if (useDrawer) setSidebarOpen(false);
  }, [useDrawer]);
  const openChannelSearch = useCallback(() => {
    setSearchChannelId(selectedChannel);
    setSidebarOpen(true);
  }, [selectedChannel]);

  // Swipe right from left edge => open, swipe left => close.
  useSwipeDrawer(sidebarOpen, openSidebar, closeSidebar, {
    containerRef: pageRef,
    drawerRef,
  });

  // Redirect to connect page when disconnected with no open sessions.
  // With open sessions we stay on /chat and show the reconnect overlay.
  useEffect(() => {
    if (status === "disconnected" && sessions.length === 0) {
      navigate("/");
    }
  }, [status, sessions.length, navigate]);

  // On mobile, block the Android swipe-back gesture / hardware back button
  // from navigating away from the chat page (which would break the connection).
  // We push a sentinel history entry and suppress any popstate that tries to
  // leave while we are still connected.
  useEffect(() => {
    if (!isMobile || status !== "connected") return;

    // Push a guard entry so there is always something to "go back" to.
    window.history.pushState({ chatGuard: true }, "");

    const onPopState = () => {
      // Re-push the guard entry to stay on the chat page.
      window.history.pushState({ chatGuard: true }, "");
    };

    window.addEventListener("popstate", onPopState);
    return () => window.removeEventListener("popstate", onPopState);
  }, [isMobile, status]);

  const handleReconnect = useCallback(async () => {
    const meta = sessions.find((s) => s.id === activeServerId);
    if (!meta) return;
    setIsReconnecting(true);
    try {
      // Remove the dead session first so reconnect creates a fresh one.
      await invoke("disconnect_server", { serverId: meta.id });
      await refreshSessions();
      await connect(meta.host, meta.port, meta.username, meta.certLabel);
    } finally {
      setIsReconnecting(false);
    }
  }, [sessions, activeServerId, connect, refreshSessions]);

  if (status === "disconnected" && sessions.length > 0) {
    const meta = sessions.find((s) => s.id === activeServerId);
    const serverLabel = meta?.label || meta?.host || "Server";
    const title = error ? "Disconnected" : "Connection lost";
    return (
      <div className={styles.reconnectPage}>
        <div className={styles.reconnectCard}>
          <div className={styles.reconnectIcon}>!</div>
          <h2 className={styles.reconnectTitle}>{title}</h2>
          <p className={styles.reconnectServer}>{serverLabel}</p>
          {error && (
            <div className={styles.reconnectReasonBox}>
              <span className={styles.reconnectReasonLabel}>Reason</span>
              <p className={styles.reconnectError}>{error}</p>
            </div>
          )}
          <button
            type="button"
            className={styles.reconnectBtn}
            onClick={() => void handleReconnect()}
            disabled={isReconnecting}
          >
            {isReconnecting ? "Reconnecting..." : "Reconnect"}
          </button>
        </div>
        <PasswordDialog
          open={passwordRequired}
          onSubmit={handlePasswordSubmit}
          onCancel={dismissPasswordPrompt}
          serverHost={pendingConnect?.host}
          username={pendingConnect?.username}
          error={error}
          showSaveOption={showSaveOption}
          onChangeUsername={handleChangeUsername}
        />
      </div>
    );
  }

  return (
    <div ref={pageRef} className={styles.page}>
      {/* Burger toggle - shown when drawer mode is active and sidebar is closed */}
      {useDrawer && !sidebarOpen && (
        <button
          className={styles.menuToggle}
          onClick={toggleSidebar}
          aria-label="Open channels"
        >
          <MenuIcon width={24} height={24} />
        </button>
      )}

      {/* Backdrop overlay when drawer is open */}
      {useDrawer && sidebarOpen && (
        <button
          className={styles.backdrop}
          onClick={closeSidebar}
          onKeyDown={(e) => e.key === "Escape" && closeSidebar()}
          aria-label="Close channels"
          type="button"
        />
      )}

      {/* Sidebar: inline on wide desktop, slide-out drawer when narrow or mobile */}
      <div
        ref={drawerRef}
        className={`${styles.sidebarContainer} ${sidebarOpen ? styles.sidebarOpen : ""}`}
      >
        <ChannelSidebar
          onChannelSelect={closeSidebar}
          onServerInfoToggle={toggleServerInfo}
          onCollapse={useDrawer ? closeSidebar : undefined}
          searchChannelId={searchChannelId}
          onSearchChannelClear={() => setSearchChannelId(null)}
        />
      </div>

      <ChatView onChannelInfoToggle={toggleChannelInfo} onChannelSearch={openChannelSearch} />
      <Suspense fallback={null}>
        {showServerInfo && !isMobile && <ServerInfoPanel onClose={() => setShowServerInfo(false)} />}
        {showChannelInfo && !isMobile && <ChannelInfoPanel onClose={() => setShowChannelInfo(false)} />}
        {(selectedUser !== null || selectedDmUser !== null) && !showServerInfo && !showChannelInfo && !isMobile && <UserProfileView />}
        {isMobile && (
          <>
            <MobileProfileSheet />
            <MobileBottomSheet
              open={showServerInfo}
              onClose={() => setShowServerInfo(false)}
              ariaLabel="Close server info"
            >
              <ServerInfoPanel onClose={() => setShowServerInfo(false)} />
            </MobileBottomSheet>
            <MobileBottomSheet
              open={showChannelInfo}
              onClose={() => setShowChannelInfo(false)}
              ariaLabel="Close channel info"
            >
              <ChannelInfoPanel onClose={() => setShowChannelInfo(false)} />
            </MobileBottomSheet>
          </>
        )}
      </Suspense>
    </div>
  );
}
