import { useEffect, useState, useCallback } from "react";
import { useNavigate } from "react-router-dom";
import { useAppStore } from "../store";
import { isMobilePlatform } from "../utils/platform";
import ChannelSidebar from "../components/ChannelSidebar";
import ChatView from "../components/ChatView";
import UserProfileView from "../components/UserProfileView";
import styles from "./ChatPage.module.css";

export default function ChatPage() {
  const status = useAppStore((s) => s.status);
  const selectedUser = useAppStore((s) => s.selectedUser);
  const navigate = useNavigate();
  const isMobile = isMobilePlatform();

  // On mobile, the sidebar is a slide-out drawer.
  const [sidebarOpen, setSidebarOpen] = useState(!isMobile);

  const toggleSidebar = useCallback(() => setSidebarOpen((v) => !v), []);
  const closeSidebar = useCallback(() => {
    if (isMobile) setSidebarOpen(false);
  }, [isMobile]);

  // Redirect to connect page if not connected.
  useEffect(() => {
    if (status === "disconnected") {
      navigate("/");
    }
  }, [status, navigate]);

  return (
    <div className={styles.page}>
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
        className={`${styles.sidebarContainer} ${sidebarOpen ? styles.sidebarOpen : ""}`}
      >
        <ChannelSidebar onChannelSelect={closeSidebar} />
      </div>

      <ChatView />
      {selectedUser !== null && !isMobile && <UserProfileView />}
    </div>
  );
}
