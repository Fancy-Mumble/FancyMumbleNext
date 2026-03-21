import { useEffect, useState, useCallback, useRef } from "react";
import { useNavigate } from "react-router-dom";
import { useAppStore } from "../store";
import { isMobilePlatform } from "../utils/platform";
import { useSwipeDrawer } from "../hooks/useSwipeDrawer";
import ChannelSidebar from "../components/ChannelSidebar";
import ChatView from "../components/ChatView";
import ServerInfoPanel from "../components/ServerInfoPanel";
import ChannelInfoPanel from "../components/ChannelInfoPanel";
import UserProfileView from "../components/UserProfileView";
import MobileProfileSheet from "../components/MobileProfileSheet";
import MobileBottomSheet from "../components/MobileBottomSheet";
import styles from "./ChatPage.module.css";

export default function ChatPage() {
  const status = useAppStore((s) => s.status);
  const selectedUser = useAppStore((s) => s.selectedUser);
  const selectedDmUser = useAppStore((s) => s.selectedDmUser);
  const navigate = useNavigate();
  const isMobile = isMobilePlatform();

  // On mobile, the sidebar is a slide-out drawer.
  const [sidebarOpen, setSidebarOpen] = useState(!isMobile);
  const pageRef = useRef<HTMLDivElement>(null);
  const drawerRef = useRef<HTMLDivElement>(null);

  const [showServerInfo, setShowServerInfo] = useState(false);
  const [showChannelInfo, setShowChannelInfo] = useState(false);

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
    if (isMobile) setSidebarOpen(false);
  }, [isMobile]);

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

  return (
    <div ref={pageRef} className={styles.page}>
      {/* Mobile hamburger toggle */}
      {isMobile && !sidebarOpen && (
        <button
          className={styles.menuToggle}
          onClick={toggleSidebar}
          aria-label="Open channels"
        >
          <svg width="24" height="24" viewBox="0 0 24 24" fill="none">
            <path d="M3 6h18M3 12h18M3 18h18" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
          </svg>
        </button>
      )}

      {/* Backdrop overlay for mobile drawer */}
      {isMobile && sidebarOpen && (
        <button
          className={styles.backdrop}
          onClick={closeSidebar}
          onKeyDown={(e) => e.key === "Escape" && closeSidebar()}
          aria-label="Close channels"
          type="button"
        />
      )}

      {/* Sidebar: always visible on desktop, drawer on mobile */}
      <div
        ref={drawerRef}
        className={`${styles.sidebarContainer} ${sidebarOpen ? styles.sidebarOpen : ""}`}
      >
        <ChannelSidebar onChannelSelect={closeSidebar} onServerInfoToggle={toggleServerInfo} />
      </div>

      <ChatView onChannelInfoToggle={toggleChannelInfo} />
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
