import { useEffect, useState, useCallback, useRef } from "react";
import { useNavigate } from "react-router-dom";
import { useAppStore } from "../store";
import { isMobile } from "../utils/platform";
import { useSwipeDrawer } from "../hooks/useSwipeDrawer";
import ChannelSidebar from "../components/sidebar/ChannelSidebar";
import ChatView from "../components/chat/ChatView";
import ServerInfoPanel from "../components/server/ServerInfoPanel";
import ChannelInfoPanel from "../components/sidebar/ChannelInfoPanel";
import UserProfileView from "../components/user/UserProfileView";
import MobileProfileSheet from "../components/user/MobileProfileSheet";
import MobileBottomSheet from "../components/elements/MobileBottomSheet";
import MenuIcon from "../assets/icons/navigation/menu.svg?react";
import styles from "./ChatPage.module.css";

export default function ChatPage() {
  const status = useAppStore((s) => s.status);
  const selectedChannel = useAppStore((s) => s.selectedChannel);
  const selectedUser = useAppStore((s) => s.selectedUser);
  const selectedDmUser = useAppStore((s) => s.selectedDmUser);
  const navigate = useNavigate();


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

  // Redirect to connect page if not connected.
  useEffect(() => {
    if (status === "disconnected") {
      navigate("/");
    }
  }, [status, navigate]);

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
    </div>
  );
}
