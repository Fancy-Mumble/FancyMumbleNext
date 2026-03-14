/**
 * MobileProfileSheet - a bottom-sheet overlay that slides up to show a
 * user's ProfilePreviewCard on mobile.  Supports swipe-down-to-dismiss.
 */

import { useCallback, useEffect, useRef, useMemo } from "react";
import { useAppStore } from "../store";
import { textureToDataUrl, parseComment } from "../profileFormat";
import { ProfilePreviewCard } from "../pages/settings/ProfilePreviewCard";
import styles from "./MobileProfileSheet.module.css";

export default function MobileProfileSheet() {
  const selectedUser = useAppStore((s) => s.selectedUser);
  const users = useAppStore((s) => s.users);
  const selectUser = useAppStore((s) => s.selectUser);

  const sheetRef = useRef<HTMLDivElement>(null);

  const user = useMemo(
    () => users.find((u) => u.session === selectedUser) ?? null,
    [users, selectedUser],
  );

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

  const dismiss = useCallback(() => selectUser(null), [selectUser]);

  // Close on Escape key.
  useEffect(() => {
    if (selectedUser === null) return;
    const handleKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") dismiss();
    };
    globalThis.addEventListener("keydown", handleKey);
    return () => globalThis.removeEventListener("keydown", handleKey);
  }, [selectedUser, dismiss]);

  // --- swipe-down-to-dismiss ---
  useEffect(() => {
    if (selectedUser === null) return;
    const el = sheetRef.current;
    if (!el) return;

    let startY = 0;
    let dragging = false;

    const onStart = (e: TouchEvent) => {
      // Only start from near the top of the sheet (handle area)
      // or when the sheet is scrolled to the top.
      if (el.scrollTop > 0) return;
      const touch = e.touches[0];
      if (!touch) return;
      startY = touch.clientY;
      dragging = true;
    };

    const onMove = (e: TouchEvent) => {
      if (!dragging) return;
      const touch = e.touches[0];
      if (!touch) return;
      const dy = touch.clientY - startY;
      if (dy < 0) {
        // Scrolling up - reset
        el.style.transition = "none";
        el.style.transform = "";
        return;
      }
      el.style.transition = "none";
      el.style.transform = `translateY(${dy}px)`;
    };

    const onEnd = (e: TouchEvent) => {
      if (!dragging) return;
      dragging = false;
      const touch = e.changedTouches[0];
      if (!touch) {
        el.style.transition = "";
        el.style.transform = "";
        return;
      }
      const dy = touch.clientY - startY;
      el.style.transition = "";
      el.style.transform = "";
      // Dismiss if dragged down more than 100px
      if (dy > 100) {
        dismiss();
      }
    };

    el.addEventListener("touchstart", onStart, { passive: true });
    el.addEventListener("touchmove", onMove, { passive: true });
    el.addEventListener("touchend", onEnd, { passive: true });

    return () => {
      el.removeEventListener("touchstart", onStart);
      el.removeEventListener("touchmove", onMove);
      el.removeEventListener("touchend", onEnd);
    };
  }, [selectedUser, dismiss]);

  if (selectedUser === null || !user) return null;

  return (
    <div className={styles.overlay}>
      <button
        className={styles.backdropBtn}
        onClick={dismiss}
        aria-label="Close profile"
        type="button"
      />
      <div ref={sheetRef} className={styles.sheet}>
        <div className={styles.handle}>
          <div className={styles.handleBar} />
        </div>
        <div className={styles.cardWrap}>
          <ProfilePreviewCard
            profile={parsed?.profile ?? {}}
            bio={parsed?.bio ?? ""}
            avatar={avatar}
            displayName={user.name}
          />
        </div>
      </div>
    </div>
  );
}
