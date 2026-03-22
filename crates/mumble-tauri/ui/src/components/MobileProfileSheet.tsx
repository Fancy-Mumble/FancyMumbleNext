/**
 * MobileProfileSheet - a bottom-sheet overlay that slides up to show a
 * user's ProfilePreviewCard on mobile.  Uses MobileBottomSheet for the
 * sheet chrome (backdrop, swipe-to-dismiss, handle).
 */

import { useMemo } from "react";
import { useAppStore } from "../store";
import { textureToDataUrl, parseComment } from "../profileFormat";
import { useUserStats } from "../hooks/useUserStats";
import { ProfilePreviewCard } from "../pages/settings/ProfilePreviewCard";
import MobileBottomSheet from "./MobileBottomSheet";
import styles from "./MobileProfileSheet.module.css";

export default function MobileProfileSheet() {
  const selectedUser = useAppStore((s) => s.selectedUser);
  const users = useAppStore((s) => s.users);
  const selectUser = useAppStore((s) => s.selectUser);

  const user = useMemo(
    () => users.find((u) => u.session === selectedUser) ?? null,
    [users, selectedUser],
  );

  const isOpen = selectedUser !== null && user !== null;
  const stats = useUserStats(selectedUser, isOpen);

  const parsed = useMemo(
    () => (user?.comment ? parseComment(user.comment) : null),
    [user?.comment],
  );

  const avatar = useMemo(
    () =>
      user?.texture && user.texture.length > 0
        ? textureToDataUrl(user.texture)
        : null,
    [user?.texture],
  );

  return (
    <MobileBottomSheet
      open={isOpen}
      onClose={() => selectUser(null)}
      ariaLabel="Close profile"
    >
      <div className={styles.cardWrap}>
        <ProfilePreviewCard
          profile={parsed?.profile ?? {}}
          bio={parsed?.bio ?? ""}
          avatar={avatar}
          displayName={user?.name ?? ""}
          onlinesecs={stats?.onlinesecs}
          idlesecs={stats?.idlesecs}
          isRegistered={user?.user_id != null && (user?.user_id ?? 0) > 0}
        />
      </div>
    </MobileBottomSheet>
  );
}
