import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useNavigate, useParams } from "react-router-dom";
import { useAppStore } from "../../store";
import { TabbedPage, type TabDef } from "../../components/elements/TabbedPage";
import type { AclGroup, RegisteredUser } from "../../types";
import { useChannelAcl } from "./useChannelAcl";
import { rootChannelId } from "./rootChannel";
import { RoleDisplayPanel } from "./RoleDisplayPanel";
import { RolePermissionsPanel } from "./RolePermissionsPanel";
import { RoleMembersPanel } from "./RoleMembersPanel";
import styles from "./AdminPanel.module.css";

type SubTab = "display" | "permissions" | "members";

const SUB_TABS: TabDef<SubTab>[] = [
  { id: "display", label: "Display", icon: "\uD83C\uDFA8" },
  { id: "permissions", label: "Permissions", icon: "\uD83D\uDD12" },
  { id: "members", label: "Members", icon: "\uD83D\uDC65" },
];

export default function RoleEditorPage() {
  const { groupName: encodedName = "" } = useParams<{ groupName: string }>();
  const navigate = useNavigate();
  const channels = useAppStore((s) => s.channels);
  const rootId = useMemo(() => rootChannelId(channels), [channels]);
  const { acl, loading, dirty, saving, setAcl, save } = useChannelAcl(rootId);
  const [tab, setTab] = useState<SubTab>("display");
  const [registeredUsers, setRegisteredUsers] = useState<RegisteredUser[]>([]);

  useEffect(() => {
    const unlisten = listen<RegisteredUser[]>("user-list", (e) => setRegisteredUsers(e.payload));
    invoke("request_user_list").catch(() => {});
    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  const roleName = decodeURIComponent(encodedName);
  const roleIdx = useMemo(() => acl?.groups.findIndex((g) => g.name === roleName) ?? -1, [acl, roleName]);
  const role: AclGroup | null = roleIdx === -1 ? null : (acl?.groups[roleIdx] ?? null);

  const patchRole = (patch: Partial<AclGroup>) => {
    if (!acl || roleIdx === -1) return;
    const groups = acl.groups.map((g, i) => (i === roleIdx ? { ...g, ...patch } : g));
    setAcl({ ...acl, groups });
  };

  const handleDelete = async () => {
    if (!acl || roleIdx === -1) return;
    const next = { ...acl, groups: acl.groups.filter((_, i) => i !== roleIdx) };
    setAcl(next);
    await save();
    navigate("/admin");
  };

  let body: React.ReactNode;
  if (loading && !acl) {
    body = <div className={styles.dimText}>Loading role...</div>;
  } else if (!role) {
    body = (
      <div className={styles.dimText}>
        Role <code>{roleName}</code> not found.
      </div>
    );
  } else if (tab === "display") {
    body = <RoleDisplayPanel role={role} onPatch={patchRole} />;
  } else if (tab === "permissions" && acl) {
    body = (
      <RolePermissionsPanel
        acl={acl}
        roleName={role.name}
        onAclChange={setAcl}
      />
    );
  } else {
    body = (
      <RoleMembersPanel
        role={role}
        onPatch={patchRole}
        registeredUsers={registeredUsers}
      />
    );
  }

  return (
    <TabbedPage
      heading={`Role: ${roleName}`}
      tabs={SUB_TABS}
      activeTab={tab}
      onTabChange={setTab}
      onBack={() => navigate("/admin")}
    >
      <div className={styles.content}>
        <div className={styles.editorActions}>
          {dirty && (
            <button
              type="button"
              className={styles.saveBtn}
              onClick={() => save()}
              disabled={saving}
            >
              {saving ? "Saving..." : "Save changes"}
            </button>
          )}
        </div>
        {body}
        {role && !role.inherited && (
          <div className={styles.editorDangerZone}>
            <button
              type="button"
              className={`${styles.dangerBtn} ${styles.dangerBtnFull}`}
              onClick={handleDelete}
              disabled={saving}
            >
              Delete role
            </button>
          </div>
        )}
      </div>
    </TabbedPage>
  );
}
