/**
 * Focused stream view - wraps ScreenShareViewer with:
 *
 * - A bottom drawer (arrow-up toggle) showing scrollable thumbnails of
 *   other concurrent streams so the user can switch.
 * - A top-right grid-layout picker (Win11 snap style) offering Solo,
 *   Side-by-side, Picture-in-picture, and 2x2 grid arrangements.
 *   Secondary panes show live-updating thumbnails of other broadcasters.
 * - Drag-and-drop reordering of both the drawer strip and grid panes.
 *   Dropping onto the primary pane switches the focused stream.
 */
import { useState, useEffect, useRef, useCallback, memo } from "react";
import ScreenShareViewer from "./ScreenShareViewer";
import { useStreamThumbnail } from "./useStreamPreview";
import ScreenShareIcon from "../../assets/icons/communication/screen-share.svg?react";
import ChevronDownIcon from "../../assets/icons/navigation/chevron-down.svg?react";
import styles from "./StreamFocusView.module.css";
import { useBroadcasterOrder, useDragStream } from "./useStreamDrag";
import type { Broadcaster, PointerDragItemProps, PointerDragHandlers } from "./useStreamDrag";

// ---------------------------------------------------------------------------
// Layout types
// ---------------------------------------------------------------------------

type GridLayout = "solo" | "side-by-side" | "pip" | "2x2" | "main+2" | "main+3";

const LAYOUT_CSS: Record<GridLayout, string> = {
  "solo":       styles.layoutSolo,
  "side-by-side": styles.layoutSideBySide,
  "pip":        styles.layoutPip,
  "2x2":        styles.layout2x2,
  "main+2":     styles.layoutMain2,
  "main+3":     styles.layoutMain3,
};

const LAYOUT_OPTIONS: { id: GridLayout; label: string }[] = [
  { id: "solo",         label: "Solo" },
  { id: "side-by-side", label: "Side by side" },
  { id: "pip",          label: "Picture-in-picture" },
  { id: "2x2",          label: "2x2 Grid" },
  { id: "main+2",       label: "Main + 2" },
  { id: "main+3",       label: "Main + 3" },
];

// ---------------------------------------------------------------------------

function LayoutVisual({ id }: { readonly id: GridLayout }) {
  switch (id) {
    case "solo":
      return (
        <div className={styles.layoutVisual}>
          <div className={`${styles.lvBlock} ${styles.lvFull}`} />
        </div>
      );
    case "side-by-side":
      return (
        <div className={styles.layoutVisual}>
          <div className={styles.lvBlock} />
          <div className={styles.lvBlock} />
        </div>
      );
    case "pip":
      return (
        <div className={`${styles.layoutVisual} ${styles.lvPipLayout}`}>
          <div className={`${styles.lvBlock} ${styles.lvMain}`} />
          <div className={`${styles.lvBlock} ${styles.lvOverlay}`} />
        </div>
      );
    case "2x2":
      return (
        <div className={`${styles.layoutVisual} ${styles.lvGrid2x2}`}>
          <div className={styles.lvBlock} />
          <div className={styles.lvBlock} />
          <div className={styles.lvBlock} />
          <div className={styles.lvBlock} />
        </div>
      );
    case "main+2":
      return (
        <div className={styles.layoutVisual}>
          <div className={`${styles.lvBlock} ${styles.lvMainLeft}`} />
          <div className={styles.lvSideColumn}>
            <div className={styles.lvBlock} />
            <div className={styles.lvBlock} />
          </div>
        </div>
      );
    case "main+3":
      return (
        <div className={styles.layoutVisual}>
          <div className={`${styles.lvBlock} ${styles.lvMainLeft}`} />
          <div className={styles.lvSideColumn}>
            <div className={styles.lvBlock} />
            <div className={styles.lvBlock} />
            <div className={styles.lvBlock} />
          </div>
        </div>
      );
  }
}

// Small 2x2 grid SVG icon for the layout picker button
function GridIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor" aria-hidden="true">
      <rect x="1" y="1" width="6" height="6" rx="1" />
      <rect x="9" y="1" width="6" height="6" rx="1" />
      <rect x="1" y="9" width="6" height="6" rx="1" />
      <rect x="9" y="9" width="6" height="6" rx="1" />
    </svg>
  );
}

// ---------------------------------------------------------------------------
// Secondary stream panel (thumbnail of a non-focused broadcaster)
// ---------------------------------------------------------------------------

interface SecondaryPanelProps extends PointerDragItemProps {
  readonly session: number;
  readonly name: string;
  readonly className?: string;
}

const SecondaryPanel = memo(function SecondaryPanel({ session, name, className, isDragOver, onItemPointerDown, onItemClick }: SecondaryPanelProps) {
  const thumbnail = useStreamThumbnail(session, true);

  return (
    <button
      type="button"
      data-drop-zone={String(session)}
      className={`${styles.secondaryPanel} ${isDragOver ? styles.dragOver : ""} ${className ?? ""}`}
      onClick={() => onItemClick(session)}
      onPointerDown={(e) => onItemPointerDown(e, session)}
      style={{ touchAction: "none" }}
      aria-label={`Switch to ${name}'s stream`}
    >
      {thumbnail
        ? <img src={thumbnail} alt="" className={styles.secondaryImg} />
        : (
          <div className={styles.secondaryPlaceholder}>
            <ScreenShareIcon width={28} height={28} />
          </div>
        )}

      <div className={styles.secondaryOverlay}>
        <span className={styles.secondaryName}>{name}</span>
        <span className={styles.secondaryHint}>Drag or click to switch</span>
      </div>
    </button>
  );
});

// ---------------------------------------------------------------------------
// Bottom drawer thumbnail strip item
// ---------------------------------------------------------------------------

interface DrawerThumbProps extends PointerDragItemProps {
  readonly session: number;
  readonly name: string;
}

const DrawerThumb = memo(function DrawerThumb({ session, name, isDragOver, onItemPointerDown, onItemClick }: DrawerThumbProps) {
  const thumbnail = useStreamThumbnail(session, true);

  return (
    <button
      type="button"
      data-drop-zone={String(session)}
      className={`${styles.drawerThumb} ${isDragOver ? styles.drawerThumbDragOver : ""}`}
      onClick={() => onItemClick(session)}
      onPointerDown={(e) => onItemPointerDown(e, session)}
      style={{ touchAction: "none" }}
      aria-label={`Watch ${name}`}
    >
      <div className={styles.drawerThumbImg}>
        {thumbnail
          ? <img src={thumbnail} alt="" className={styles.drawerThumbImgEl} />
          : (
            <div className={styles.drawerThumbPlaceholder}>
              <ScreenShareIcon width={20} height={20} />
            </div>
          )}
        <span className={styles.drawerLiveBadge}>LIVE</span>
      </div>
      <span className={styles.drawerThumbName}>{name}</span>
    </button>
  );
});

// ---------------------------------------------------------------------------
// Primary stream pane - handles drop zone and visual feedback
// ---------------------------------------------------------------------------

interface PrimaryPaneProps {
  readonly isOwnBroadcast: boolean;
  readonly localStream: MediaStream | null;
  readonly session?: number;
  readonly hasOthers: boolean;
  readonly isPrimaryDragOver: boolean;
}

function PrimaryPane({ isOwnBroadcast, localStream, session, hasOthers, isPrimaryDragOver }: PrimaryPaneProps) {
  return (
    <section
      className={styles.primaryPane}
      aria-label="Primary stream"
      data-drop-zone={hasOthers ? "primary" : undefined}
    >
      <ScreenShareViewer isOwnBroadcast={isOwnBroadcast} localStream={localStream} session={session} />
      {hasOthers && (
        <div
          className={`${styles.primaryDropZone} ${isPrimaryDragOver ? styles.primaryDropZoneActive : ""}`}
          aria-hidden="true"
        >
          {isPrimaryDragOver && (
            <span className={styles.primaryDropLabel}>Drop to focus</span>
          )}
        </div>
      )}
    </section>
  );
}

// ---------------------------------------------------------------------------
// Layout picker bar (top-right)
// ---------------------------------------------------------------------------

interface LayoutPickerBarProps {
  readonly layout: GridLayout;
  readonly onSelectLayout: (id: GridLayout) => void;
  readonly pickerRef: React.RefObject<HTMLDivElement | null>;
}

function LayoutPickerBar({ layout, onSelectLayout, pickerRef }: LayoutPickerBarProps) {
  const [open, setOpen] = useState(false);

  useEffect(() => {
    if (!open) return;
    const handlePointerDown = (e: MouseEvent) => {
      if (!pickerRef.current?.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", handlePointerDown);
    return () => document.removeEventListener("mousedown", handlePointerDown);
  }, [open, pickerRef]);

  const handleSelect = useCallback((id: GridLayout) => {
    onSelectLayout(id);
    setOpen(false);
  }, [onSelectLayout]);

  return (
    <div className={styles.topBar} ref={pickerRef}>
      <button
        type="button"
        className={`${styles.layoutBtn} ${open ? styles.layoutBtnActive : ""}`}
        onClick={() => setOpen((p) => !p)}
        title="Change layout"
        aria-label="Change layout"
        aria-expanded={open}
      >
        <GridIcon />
      </button>
      {open && (
        <div className={styles.layoutPicker} role="menu">
          <div className={styles.layoutPickerTitle}>Snap layout</div>
          <div className={styles.layoutPickerGrid}>
            {LAYOUT_OPTIONS.map((opt) => (
              <button
                key={opt.id}
                type="button"
                role="menuitem"
                className={`${styles.layoutOption} ${layout === opt.id ? styles.layoutOptionActive : ""}`}
                onClick={() => handleSelect(opt.id)}
                title={opt.label}
                aria-label={opt.label}
                aria-current={layout === opt.id ? "true" : undefined}
              >
                <LayoutVisual id={opt.id} />
                <span className={styles.layoutOptionLabel}>{opt.label}</span>
              </button>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Bottom drawer - collapsible thumbnail strip
// ---------------------------------------------------------------------------

interface StreamDrawerProps {
  readonly broadcasters: Broadcaster[];
  readonly orderedList: Broadcaster[];
  readonly dragOverTarget: number | "primary" | null;
  readonly dragHandlers: PointerDragHandlers;
}

function StreamDrawer({ broadcasters, orderedList, dragOverTarget, dragHandlers }: StreamDrawerProps) {
  const [open, setOpen] = useState(false);
  const toggle = useCallback(() => setOpen((p) => !p), []);

  return (
    <div className={`${styles.drawer} ${open ? styles.drawerOpen : ""}`}>
      <button
        type="button"
        className={styles.drawerToggle}
        onClick={toggle}
        title={open ? "Hide other streams" : "Show other streams"}
        aria-label={open ? "Hide other streams" : "Show other streams"}
        aria-expanded={open}
      >
        <ChevronDownIcon
          width={14}
          height={14}
          className={`${styles.toggleChevron} ${open ? styles.chevronFlipped : ""}`}
        />
        {!open && (
          <span className={styles.toggleLabel}>
            {broadcasters.length} other stream{broadcasters.length === 1 ? "" : "s"}
          </span>
        )}
      </button>
      <div className={styles.drawerStrip} aria-hidden={!open}>
        {orderedList.map((b) => (
          <DrawerThumb
            key={b.session}
            session={b.session}
            name={b.name}
            isDragOver={dragOverTarget === b.session}
            {...dragHandlers}
          />
        ))}
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Secondary pane layout rendering
// ---------------------------------------------------------------------------

interface LayoutSecondaryPanesProps {
  readonly layout: GridLayout;
  readonly secondaries: Broadcaster[];
  readonly dragOverTarget: number | "primary" | null;
  readonly dragHandlers: PointerDragHandlers;
}

function LayoutSecondaryPanes({ layout, secondaries, dragOverTarget, dragHandlers }: LayoutSecondaryPanesProps) {
  if (layout === "side-by-side" && secondaries[0]) {
    return (
      <SecondaryPanel
        session={secondaries[0].session}
        name={secondaries[0].name}
        isDragOver={dragOverTarget === secondaries[0].session}
        {...dragHandlers}
      />
    );
  }
  if (layout === "pip" && secondaries[0]) {
    return (
      <SecondaryPanel
        session={secondaries[0].session}
        name={secondaries[0].name}
        className={styles.pipPanel}
        isDragOver={dragOverTarget === secondaries[0].session}
        {...dragHandlers}
      />
    );
  }
  if (layout === "2x2") {
    return (
      <>
        {secondaries.map((b) => (
          <SecondaryPanel
            key={b.session}
            session={b.session}
            name={b.name}
            isDragOver={dragOverTarget === b.session}
            {...dragHandlers}
          />
        ))}
      </>
    );
  }
  if ((layout === "main+2" || layout === "main+3") && secondaries.length > 0) {
    return (
      <div className={styles.rightColumn}>
        {secondaries.map((b) => (
          <SecondaryPanel
            key={b.session}
            session={b.session}
            name={b.name}
            isDragOver={dragOverTarget === b.session}
            {...dragHandlers}
          />
        ))}
      </div>
    );
  }
  return null;
}

// ---------------------------------------------------------------------------
// Main component
// ---------------------------------------------------------------------------

interface StreamFocusViewProps {
  readonly isOwnBroadcast: boolean;
  readonly localStream: MediaStream | null;
  /** Session ID of the broadcaster shown in the primary pane. */
  readonly session?: number;
  readonly otherBroadcasters: Broadcaster[];
  readonly onWatch: (session: number) => void;
}

export default function StreamFocusView({
  isOwnBroadcast,
  localStream,
  session,
  otherBroadcasters,
  onWatch,
}: StreamFocusViewProps) {
  const [layout, setLayout] = useState<GridLayout>("solo");
  const pickerRef = useRef<HTMLDivElement>(null);

  const { orderedList, reorder } = useBroadcasterOrder(otherBroadcasters);
  const { dragOverTarget, dragHandlers } = useDragStream(onWatch, reorder);

  // Base hasOthers on the prop directly, not on orderedList.  The internal
  // order state in useBroadcasterOrder updates one render after the prop
  // changes, so orderedList is transiently empty during the transition and
  // would wrongly trigger the layout reset to "solo".
  const hasOthers = otherBroadcasters.length > 0;
  const secondaries = orderedList.slice(0, layout === "main+2" ? 2 : 3);

  useEffect(() => {
    if (!hasOthers) setLayout("solo");
  }, [hasOthers]);

  const selectLayout = useCallback((id: GridLayout) => setLayout(id), []);

  return (
    <div className={styles.container}>
      <div className={`${styles.videoArea} ${LAYOUT_CSS[layout]}`}>
        <PrimaryPane
          isOwnBroadcast={isOwnBroadcast}
          localStream={localStream}
          session={session}
          hasOthers={hasOthers}
          isPrimaryDragOver={dragOverTarget === "primary"}
        />

        <LayoutSecondaryPanes
          layout={layout}
          secondaries={secondaries}
          dragOverTarget={dragOverTarget}
          dragHandlers={dragHandlers}
        />
      </div>

      {hasOthers && (
        <LayoutPickerBar
          layout={layout}
          onSelectLayout={selectLayout}
          pickerRef={pickerRef}
        />
      )}

      {hasOthers && (
        <StreamDrawer
          broadcasters={otherBroadcasters}
          orderedList={orderedList}
          dragOverTarget={dragOverTarget}
          dragHandlers={dragHandlers}
        />
      )}
    </div>
  );
}
