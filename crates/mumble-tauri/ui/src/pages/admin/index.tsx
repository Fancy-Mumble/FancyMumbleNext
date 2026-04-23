import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { TabbedPage, type TabDef } from "../../components/elements/TabbedPage";
import { RegisteredUsersTab } from "./RegisteredUsersTab";
import { BanListTab } from "./BanListTab";
import { ChannelAclTab } from "./ChannelAclTab";
import { RolesListPanel } from "./RolesListPanel";
import styles from "./AdminPanel.module.css";

type Tab = "users" | "roles" | "bans" | "acl";

const TABS: TabDef<Tab>[] = [
  { id: "users", label: "Users", icon: "\uD83D\uDC65" },
  { id: "roles", label: "Roles", icon: "\uD83C\uDFAD" },
  { id: "bans", label: "Ban List", icon: "\uD83D\uDEAB" },
  { id: "acl", label: "Channel ACL", icon: "\uD83D\uDD12" },
];

export default function AdminPanel() {
  const navigate = useNavigate();
  const [tab, setTab] = useState<Tab>("users");

  return (
    <TabbedPage
      heading="Admin"
      tabs={TABS}
      activeTab={tab}
      onTabChange={setTab}
      onBack={() => navigate("/chat")}
    >
      <div className={styles.content}>
        {tab === "users" && <RegisteredUsersTab />}
        {tab === "roles" && <RolesListPanel />}
        {tab === "bans" && <BanListTab />}
        {tab === "acl" && <ChannelAclTab />}
      </div>
    </TabbedPage>
  );
}
