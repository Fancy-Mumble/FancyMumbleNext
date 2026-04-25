import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { TabbedPage, type TabDef } from "../../components/elements/TabbedPage";
import { useAppStore } from "../../store";
import { RegisteredUsersTab } from "./RegisteredUsersTab";
import { BanListTab } from "./BanListTab";
import { ChannelAclTab } from "./ChannelAclTab";
import { RolesListPanel } from "./RolesListPanel";
import { CustomEmotesTab } from "./CustomEmotesTab";
import { PERM_MANAGE_EMOTES } from "../../utils/permissions";
import styles from "./AdminPanel.module.css";

type Tab = "users" | "roles" | "bans" | "acl" | "emotes";

const BASE_TABS: TabDef<Tab>[] = [
  { id: "users", label: "Users", icon: "\uD83D\uDC65" },
  { id: "roles", label: "Roles", icon: "\uD83C\uDFAD" },
  { id: "bans", label: "Ban List", icon: "\uD83D\uDEAB" },
  { id: "acl", label: "Channel ACL", icon: "\uD83D\uDD12" },
];

export default function AdminPanel() {
  const navigate = useNavigate();
  const [tab, setTab] = useState<Tab>("users");
  const customEmotesSupported = useAppStore((s) => s.fileServerCapabilities?.features.custom_emotes ?? false);
  const rootChannelPerms = useAppStore((s) => s.channels.find((c) => c.id === 0)?.permissions ?? 0);
  const canManageEmotes = customEmotesSupported && (rootChannelPerms & PERM_MANAGE_EMOTES) !== 0;
  const tabs: TabDef<Tab>[] = canManageEmotes
    ? [...BASE_TABS, { id: "emotes", label: "Emotes", icon: "\uD83C\uDFA8" }]
    : BASE_TABS;

  return (
    <TabbedPage
      heading="Admin"
      tabs={tabs}
      activeTab={tab}
      onTabChange={setTab}
      onBack={() => navigate("/chat")}
    >
      <div className={styles.content}>
        {tab === "users" && <RegisteredUsersTab />}
        {tab === "roles" && <RolesListPanel />}
        {tab === "bans" && <BanListTab />}
        {tab === "acl" && <ChannelAclTab />}
        {tab === "emotes" && <CustomEmotesTab />}
      </div>
    </TabbedPage>
  );
}
