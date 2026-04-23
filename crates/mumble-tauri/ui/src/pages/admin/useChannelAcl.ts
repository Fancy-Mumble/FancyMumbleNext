import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { AclData } from "../../types";

/**
 * Subscribe to the `acl` event and request the ACL for the given channel.
 *
 * The hook keeps the latest ACL snapshot in state and tracks a `dirty` flag
 * so callers can mutate the snapshot locally and persist it via `save`.
 */
export function useChannelAcl(channelId: number | null) {
  const [acl, setAcl] = useState<AclData | null>(null);
  const [loading, setLoading] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);
  const requestedFor = useRef<number | null>(null);

  useEffect(() => {
    let unlisten: UnlistenFn | null = null;
    listen<AclData>("acl", (event) => {
      if (channelId !== null && event.payload.channel_id === channelId) {
        setAcl(event.payload);
        setDirty(false);
        setLoading(false);
      }
    }).then((u) => {
      unlisten = u;
    });
    return () => {
      unlisten?.();
    };
  }, [channelId]);

  useEffect(() => {
    if (channelId === null) {
      setAcl(null);
      requestedFor.current = null;
      return;
    }
    if (requestedFor.current === channelId && acl !== null) return;
    requestedFor.current = channelId;
    setLoading(true);
    invoke("request_acl", { channelId }).catch(() => setLoading(false));
  }, [channelId, acl]);

  const update = useCallback((next: AclData) => {
    setAcl(next);
    setDirty(true);
  }, []);

  const save = useCallback(async () => {
    if (!acl) return;
    setSaving(true);
    try {
      await invoke("update_acl", { acl });
      setDirty(false);
    } finally {
      setSaving(false);
    }
  }, [acl]);

  const refresh = useCallback(() => {
    if (channelId === null) return;
    setLoading(true);
    invoke("request_acl", { channelId }).catch(() => setLoading(false));
  }, [channelId]);

  return { acl, loading, dirty, saving, setAcl: update, save, refresh } as const;
}
