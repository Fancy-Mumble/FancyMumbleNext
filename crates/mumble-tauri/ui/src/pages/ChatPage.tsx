import { useEffect } from "react";
import { useNavigate } from "react-router-dom";
import { useAppStore } from "../store";
import ChannelSidebar from "../components/ChannelSidebar";
import ChatView from "../components/ChatView";
import UserProfileView from "../components/UserProfileView";
import styles from "./ChatPage.module.css";

export default function ChatPage() {
  const status = useAppStore((s) => s.status);
  const selectedUser = useAppStore((s) => s.selectedUser);
  const navigate = useNavigate();

  // Redirect to connect page if not connected.
  useEffect(() => {
    if (status === "disconnected") {
      navigate("/");
    }
  }, [status, navigate]);

  return (
    <div className={styles.page}>
      <ChannelSidebar />
      <ChatView />
      {selectedUser !== null && <UserProfileView />}
    </div>
  );
}
