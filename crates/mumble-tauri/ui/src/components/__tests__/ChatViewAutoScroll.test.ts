/**
 * Integration-style tests for the chat auto-scroll state machine.
 *
 * Simulates the interaction between:
 *   - scroll events (from programmatic scrollTo, scrollbar drag, keyboard)
 *   - wheel events (most common desktop scroll input)
 *   - touch events (mobile / touchscreen)
 *   - message arrival (triggers auto-scroll)
 *   - image load (per-image load handler / ResizeObserver / MutationObserver)
 *
 * The core class under test (`ScrollController`) mirrors the exact logic
 * in ChatView.tsx so we can reproduce race conditions deterministically
 * without needing a real browser layout engine.
 *
 * KEY DESIGN:
 * `stickToBottom` can only be set to `false` by:
 *   - wheel event with deltaY < 0 (user scrolling up)
 *   - touch move with finger moving down (content scrolling up)
 *   - scroll event when NOT near bottom AND outside the 150ms grace
 *     window after the last programmatic scroll.
 *
 * Three independent re-pin mechanisms guarantee that content-height
 * changes (images loading, embeds, etc.) always trigger a scroll when
 * the user is at the bottom:
 *   1. ResizeObserver on the inner wrapper
 *   2. Per-image `load` handlers (attached by MutationObserver on new nodes)
 *   3. MutationObserver itself (for new DOM content from React renders)
 *
 * All re-pin calls go through a bottom sentinel element via
 * `scrollIntoView`, wrapped in rAF, so the scroll position is computed
 * after layout settles.
 */

import { describe, it, expect, beforeEach } from "vitest";

// ---------------------------------------------------------------------------
// Minimal mock of the scroll container
// ---------------------------------------------------------------------------

interface MockContainer {
  scrollHeight: number;
  scrollTop: number;
  clientHeight: number;
  /** Accumulated scrollTo calls for assertions. */
  scrollToCalls: Array<{ top: number; behavior: string }>;
  scrollTo(opts: { top: number; behavior: string }): void;
}

function createMockContainer(clientHeight = 600): MockContainer {
  const container: MockContainer = {
    scrollHeight: clientHeight,
    scrollTop: 0,
    clientHeight,
    scrollToCalls: [],
    scrollTo(opts: { top: number; behavior: string }) {
      container.scrollTop = Math.min(
        opts.top,
        Math.max(0, container.scrollHeight - container.clientHeight),
      );
      container.scrollToCalls.push({ top: opts.top, behavior: opts.behavior });
    },
  };
  return container;
}

// ---------------------------------------------------------------------------
// ScrollController - mirrors ChatView scroll logic
// ---------------------------------------------------------------------------

const NEAR_BOTTOM_PX = 120;
const GRACE_MS = 150;

function isNearBottom(c: MockContainer): boolean {
  return c.scrollHeight - c.scrollTop - c.clientHeight < NEAR_BOTTOM_PX;
}

function isWithinHalfViewport(c: MockContainer): boolean {
  const threshold = Math.max(c.clientHeight / 2, NEAR_BOTTOM_PX);
  return c.scrollHeight - c.scrollTop - c.clientHeight < threshold;
}

/**
 * Mirrors the auto-scroll state machine from ChatView.tsx.
 *
 * `stickToBottom` is the core intent flag.  It is ONLY set to false by
 * explicit user gestures.  Programmatic scrollTo + image decode races
 * cannot corrupt it because:
 *   - wheel/touch handlers are never fired by programmatic scrollTo
 *   - scroll handler ignores events within the grace window
 */
class ScrollController {
  container: MockContainer;
  stickToBottom = true;
  newMsgCount = 0;
  lastReadIdx: number | null = null;
  prevMsgCount = 0;
  /** Timestamp (ms) of the last programmatic scrollTo. */
  lastProgrammaticScroll = 0;
  /** Current simulated time (ms). */
  now = 1000;

  constructor(container: MockContainer) {
    this.container = container;
  }

  /** Simulate the scroll event listener. */
  onScroll(): void {
    const atBottom = isNearBottom(this.container);
    if (atBottom) {
      this.stickToBottom = true;
      if (this.newMsgCount > 0) {
        this.newMsgCount = 0;
        this.lastReadIdx = null;
      }
    } else if (this.now - this.lastProgrammaticScroll > GRACE_MS) {
      // Not near bottom AND outside grace window -> user scrolled away
      this.stickToBottom = false;
    }
    // Inside grace window: leave stickToBottom unchanged
  }

  /** Simulate a wheel event (deltaY < 0 = scroll up). */
  onWheel(deltaY: number): void {
    if (deltaY < 0) this.stickToBottom = false;
  }

  /** Simulate a touch-move where finger moved down (content scrolls up). */
  onTouchScrollUp(): void {
    this.stickToBottom = false;
  }

  /** Programmatic scroll-to-bottom. */
  scrollToBottom(): void {
    this.stickToBottom = true;
    this.lastProgrammaticScroll = this.now;
    this.container.scrollTo({
      top: this.container.scrollHeight,
      behavior: "instant",
    });
  }

  /** Simulates the message-count effect. */
  onMessageCountChange(msgCount: number): void {
    const delta = msgCount - this.prevMsgCount;
    this.prevMsgCount = msgCount;
    if (delta <= 0) return;

    const atBottom = isWithinHalfViewport(this.container);
    if (atBottom) {
      this.stickToBottom = true;
      this.scrollToBottom();
    } else {
      this.stickToBottom = false;
      this.lastReadIdx = this.lastReadIdx ?? msgCount - delta;
      this.newMsgCount += delta;
    }
  }

  /** Simulates the ResizeObserver / MutationObserver callback. */
  onResize(): void {
    if (this.stickToBottom) {
      this.lastProgrammaticScroll = this.now;
      this.container.scrollTo({
        top: this.container.scrollHeight,
        behavior: "instant",
      });
    }
  }

  /**
   * Simulates a per-image `load` handler firing (the image has decoded
   * and its dimensions are now reflected in scrollHeight).  This is
   * the same as `onResize` in the model but represents the per-element
   * load handler attached by the MutationObserver scan.
   */
  onImageLoad(): void {
    this.onResize();
  }

  /** Convenience: add content height (simulates an image decoding). */
  growContent(px: number): void {
    this.container.scrollHeight += px;
  }

  /** Advance simulated time. */
  advanceTime(ms: number): void {
    this.now += ms;
  }

  /** Distance from the bottom of the scroll area. */
  get distFromBottom(): number {
    return (
      this.container.scrollHeight -
      this.container.scrollTop -
      this.container.clientHeight
    );
  }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe("ChatView auto-scroll state machine", () => {
  let container: MockContainer;
  let ctrl: ScrollController;

  beforeEach(() => {
    container = createMockContainer(600);
    ctrl = new ScrollController(container);
    container.scrollHeight = 1600;
    container.scrollTop = 1000; // at exact bottom (1600 - 600)
    ctrl.prevMsgCount = 5;
  });

  // -- Basic sanity --

  it("starts at the bottom", () => {
    expect(ctrl.distFromBottom).toBe(0);
    expect(ctrl.stickToBottom).toBe(true);
  });

  it("scrollToBottom scrolls to the absolute bottom", () => {
    container.scrollHeight = 2000;
    ctrl.scrollToBottom();
    expect(container.scrollTop).toBe(1400);
    expect(ctrl.distFromBottom).toBe(0);
  });

  // -- Single image load --

  it("re-pins after a single image loads", () => {
    ctrl.growContent(240);
    expect(ctrl.distFromBottom).toBe(240);

    ctrl.onResize();
    expect(ctrl.distFromBottom).toBe(0);
  });

  // -- The classic race condition (the original bug) --

  it("handles the classic race: image B decodes between scrollTo and scroll event", () => {
    // 1. Image A loads -> ResizeObserver fires -> scrollTo bottom
    ctrl.growContent(240); // scrollHeight: 1840
    ctrl.onResize(); // scrollTo -> scrollTop: 1240
    expect(ctrl.distFromBottom).toBe(0);

    // 2. Image B decodes BEFORE the scroll event from step 1 fires
    ctrl.growContent(240); // scrollHeight: 2080

    // 3. Scroll event fires (within grace window, so stickToBottom stays true)
    ctrl.onScroll();
    expect(ctrl.stickToBottom).toBe(true);

    // 4. ResizeObserver fires for image B -> re-pins
    ctrl.onResize();
    expect(ctrl.distFromBottom).toBe(0);
  });

  it("handles three images loading in rapid succession", () => {
    // Image 1 loads
    ctrl.growContent(200); // scrollHeight: 1800
    ctrl.onResize(); // scrollTo -> scrollTop: 1200

    // Image 2 decodes before scroll event from image 1
    ctrl.growContent(200); // scrollHeight: 2000

    // Scroll event fires (within grace window)
    ctrl.onScroll();
    expect(ctrl.stickToBottom).toBe(true);

    // ResizeObserver for image 2
    ctrl.onResize(); // scrollTo -> scrollTop: 1400

    // Image 3 decodes before scroll event from image 2
    ctrl.growContent(200); // scrollHeight: 2200

    // Scroll event fires (within grace window)
    ctrl.onScroll();
    expect(ctrl.stickToBottom).toBe(true);

    // ResizeObserver for image 3
    ctrl.onResize(); // scrollTo -> scrollTop: 1600
    expect(ctrl.distFromBottom).toBe(0);
  });

  it("handles new message with image that loads slowly", () => {
    // New message text arrives
    ctrl.growContent(50); // scrollHeight: 1650
    ctrl.onMessageCountChange(6);
    expect(ctrl.distFromBottom).toBe(0);

    // Some time passes (but within grace)
    ctrl.advanceTime(50);

    // Scroll event from auto-scroll
    ctrl.onScroll();
    expect(ctrl.stickToBottom).toBe(true);

    // Image loads after 500ms
    ctrl.advanceTime(500);
    ctrl.growContent(240); // scrollHeight: 1890

    // ResizeObserver fires -> stickToBottom is true -> re-pins
    ctrl.onResize();
    expect(ctrl.distFromBottom).toBe(0);
  });

  it("handles new message followed by TWO slow images", () => {
    ctrl.growContent(50);
    ctrl.onMessageCountChange(6);
    expect(ctrl.distFromBottom).toBe(0);

    // Image 1 loads (within grace)
    ctrl.advanceTime(100);
    ctrl.growContent(240); // scrollHeight: 1890
    ctrl.onResize();
    expect(ctrl.distFromBottom).toBe(0);

    // Image 2 loads before scroll event from image 1
    ctrl.growContent(240); // scrollHeight: 2130
    ctrl.onScroll(); // within grace
    expect(ctrl.stickToBottom).toBe(true);

    // ResizeObserver for image 2
    ctrl.onResize();
    expect(ctrl.distFromBottom).toBe(0);
  });

  // -- User scroll-up should NOT be overridden --

  it("does not re-pin when user scrolls up via wheel", () => {
    // User scrolls up via mouse wheel
    ctrl.onWheel(-120);
    expect(ctrl.stickToBottom).toBe(false);

    // Simulate the actual scroll position change
    container.scrollTop = 600;

    // Image loads in an older message
    ctrl.growContent(200);
    ctrl.onResize(); // stickToBottom is false -> no scroll
    expect(container.scrollTop).toBe(600); // unchanged
  });

  it("does not re-pin when user scrolls up via touch", () => {
    ctrl.onTouchScrollUp();
    expect(ctrl.stickToBottom).toBe(false);

    container.scrollTop = 600;
    ctrl.growContent(200);
    ctrl.onResize();
    expect(container.scrollTop).toBe(600);
  });

  it("does not re-pin when user drags scrollbar (scroll event outside grace)", () => {
    // Wait for grace window to expire
    ctrl.advanceTime(200);

    // User drags scrollbar up
    container.scrollTop = 600;
    ctrl.onScroll(); // outside grace, not near bottom -> stickToBottom = false
    expect(ctrl.stickToBottom).toBe(false);

    ctrl.growContent(200);
    ctrl.onResize();
    expect(container.scrollTop).toBe(600);
  });

  it("does not auto-scroll new messages when user scrolled up past half viewport", () => {
    ctrl.onWheel(-120);
    container.scrollTop = 600;
    ctrl.onScroll();
    expect(ctrl.stickToBottom).toBe(false);

    ctrl.growContent(50);
    ctrl.onMessageCountChange(6);

    expect(ctrl.newMsgCount).toBe(1);
    expect(container.scrollTop).toBe(600);
  });

  it("does auto-scroll new messages when user is within half viewport", () => {
    container.scrollTop = 900; // distFromBottom = 100
    ctrl.advanceTime(200);
    ctrl.onScroll();
    expect(ctrl.stickToBottom).toBe(true); // within 120px

    ctrl.growContent(50);
    ctrl.onMessageCountChange(6);

    expect(ctrl.distFromBottom).toBe(0);
    expect(ctrl.newMsgCount).toBe(0);
  });

  // -- Grace window behavior --

  it("scroll event within grace window does NOT clear stickToBottom", () => {
    // Programmatic scroll
    ctrl.scrollToBottom();

    // Image decodes immediately (same frame)
    ctrl.growContent(300); // scrollHeight: 1900

    // Scroll event fires within 150ms grace -> stickToBottom stays true
    ctrl.advanceTime(50);
    ctrl.onScroll(); // 1900 - 1000 - 600 = 300 > 120, but within grace
    expect(ctrl.stickToBottom).toBe(true);

    // ResizeObserver re-pins
    ctrl.onResize();
    expect(ctrl.distFromBottom).toBe(0);
  });

  it("scroll event AFTER grace window DOES clear stickToBottom", () => {
    ctrl.scrollToBottom();
    ctrl.advanceTime(200); // past 150ms grace

    container.scrollTop = 400;
    ctrl.onScroll(); // outside grace, not near bottom -> clears
    expect(ctrl.stickToBottom).toBe(false);
  });

  // -- User scroll-up after images, then more images --

  it("respects user wheel-up even after programmatic scroll + image", () => {
    ctrl.growContent(200);
    ctrl.onResize();
    ctrl.advanceTime(50);
    ctrl.onScroll();

    // User scrolls up via wheel
    ctrl.onWheel(-120);
    container.scrollTop = 400;
    expect(ctrl.stickToBottom).toBe(false);

    // Another image loads -> should NOT re-pin
    ctrl.growContent(200);
    ctrl.onResize();
    expect(container.scrollTop).toBe(400);
  });

  // -- Pill click (jump to bottom) --

  it("pill click re-enables auto-scroll", () => {
    // User scrolls up
    ctrl.onWheel(-120);
    container.scrollTop = 200;
    expect(ctrl.stickToBottom).toBe(false);

    // New messages arrive while scrolled up
    ctrl.growContent(100);
    ctrl.onMessageCountChange(8);
    expect(ctrl.newMsgCount).toBe(3);

    // User clicks "jump to bottom" pill
    ctrl.newMsgCount = 0;
    ctrl.lastReadIdx = null;
    ctrl.scrollToBottom();
    expect(ctrl.distFromBottom).toBe(0);

    // Image loads -> should re-pin
    ctrl.growContent(240);
    ctrl.onResize();
    expect(ctrl.distFromBottom).toBe(0);
  });

  // -- Worst case: many images in a burst --

  it("handles a burst of 5 images loading with interleaved scroll events", () => {
    const IMAGE_HEIGHT = 200;
    const NUM_IMAGES = 5;

    for (let i = 0; i < NUM_IMAGES; i++) {
      ctrl.growContent(IMAGE_HEIGHT);
      ctrl.onResize();

      // Next image may load before scroll event
      if (i < NUM_IMAGES - 1) {
        ctrl.growContent(IMAGE_HEIGHT);
      }

      // Scroll event (within grace since onResize just set the timestamp)
      ctrl.advanceTime(10);
      ctrl.onScroll();
    }

    ctrl.onResize();
    ctrl.onScroll();

    expect(ctrl.distFromBottom).toBe(0);
    expect(ctrl.stickToBottom).toBe(true);
  });

  // -- Channel switch resets everything --

  it("channel switch resets state and scrolls to bottom", () => {
    ctrl.onWheel(-120);
    container.scrollTop = 200;
    ctrl.newMsgCount = 5;
    ctrl.lastReadIdx = 3;

    // Channel switch: reset
    ctrl.newMsgCount = 0;
    ctrl.lastReadIdx = null;
    ctrl.prevMsgCount = 10;
    container.scrollHeight = 3000;
    ctrl.scrollToBottom();

    expect(ctrl.distFromBottom).toBe(0);
    expect(ctrl.newMsgCount).toBe(0);
    expect(ctrl.stickToBottom).toBe(true);
  });

  // -- The exact bug scenario: showing 2nd-to-last image --

  it("does NOT get stuck at 2nd-to-last image when posting a new image", () => {
    // User posts a message with an image (text arrives first)
    ctrl.growContent(50);
    ctrl.onMessageCountChange(6);
    expect(ctrl.distFromBottom).toBe(0);

    // Scroll event fires from auto-scroll (within grace)
    ctrl.advanceTime(10);
    ctrl.onScroll();
    expect(ctrl.stickToBottom).toBe(true);

    // Image loads (slow decode, but stickToBottom is still true)
    ctrl.advanceTime(500);
    ctrl.growContent(300); // scrollHeight grew by 300

    // ResizeObserver fires -> stickToBottom is true -> re-pins
    ctrl.onResize();
    expect(ctrl.distFromBottom).toBe(0);

    // Another scroll event
    ctrl.advanceTime(10);
    ctrl.onScroll();
    expect(ctrl.stickToBottom).toBe(true);
  });

  // -- Two images same frame --

  it("handles two images loading in the same frame", () => {
    ctrl.growContent(200); // image A
    ctrl.growContent(200); // image B (same frame)

    // Single ResizeObserver callback
    ctrl.onResize();
    expect(ctrl.distFromBottom).toBe(0);
  });

  // -- Wheel down does NOT disable stickToBottom --

  it("wheel down (scrolling toward bottom) does not disable stickToBottom", () => {
    ctrl.onWheel(120); // positive deltaY = scrolling down
    expect(ctrl.stickToBottom).toBe(true);
  });

  // -- Re-enabling stickToBottom by scrolling back to bottom --

  it("scrolling back to bottom re-enables stickToBottom", () => {
    ctrl.onWheel(-120);
    container.scrollTop = 200;
    expect(ctrl.stickToBottom).toBe(false);

    // User scrolls back to bottom
    container.scrollTop = 1000;
    ctrl.advanceTime(200);
    ctrl.onScroll(); // near bottom -> stickToBottom = true
    expect(ctrl.stickToBottom).toBe(true);

    // Image loads -> should re-pin
    ctrl.growContent(240);
    ctrl.onResize();
    expect(ctrl.distFromBottom).toBe(0);
  });

  // -- Per-image load handler tests --

  it("per-image load handler re-pins after slow external image", () => {
    // New message with external URL image (starts at 0x0)
    ctrl.growContent(50); // text only
    ctrl.onMessageCountChange(6);
    expect(ctrl.distFromBottom).toBe(0);

    // 1 second later, image loads from network
    ctrl.advanceTime(1000);
    ctrl.growContent(240); // image decoded, container grows

    // Per-image load handler fires (not ResizeObserver)
    ctrl.onImageLoad();
    expect(ctrl.distFromBottom).toBe(0);
    expect(ctrl.stickToBottom).toBe(true);
  });

  it("per-image load handler and ResizeObserver both fire - no conflict", () => {
    ctrl.growContent(50);
    ctrl.onMessageCountChange(6);

    ctrl.advanceTime(500);
    ctrl.growContent(240);

    // Both handlers fire for the same image decode
    ctrl.onImageLoad();
    expect(ctrl.distFromBottom).toBe(0);

    // ResizeObserver also fires (redundant but safe)
    ctrl.onResize();
    expect(ctrl.distFromBottom).toBe(0);
    expect(ctrl.stickToBottom).toBe(true);
  });

  it("MutationObserver-triggered repin on new DOM nodes", () => {
    // React commits a new message to the DOM
    ctrl.growContent(80); // message text + placeholder

    // MutationObserver fires (childList change) -> repin
    ctrl.onResize(); // same as MutationObserver's repin call
    expect(ctrl.distFromBottom).toBe(0);

    // Image inside the new message loads later
    ctrl.advanceTime(300);
    ctrl.growContent(200);
    ctrl.onImageLoad();
    expect(ctrl.distFromBottom).toBe(0);
  });

  it("multiple external images load at different times", () => {
    // Message with 3 external images arrives
    ctrl.growContent(50); // text only
    ctrl.onMessageCountChange(6);
    expect(ctrl.distFromBottom).toBe(0);

    // Image 1 loads after 200ms
    ctrl.advanceTime(200);
    ctrl.growContent(180);
    ctrl.onImageLoad();
    expect(ctrl.distFromBottom).toBe(0);

    // Image 2 loads after 800ms
    ctrl.advanceTime(600);
    ctrl.growContent(220);
    ctrl.onImageLoad();
    expect(ctrl.distFromBottom).toBe(0);

    // Scroll event (well outside grace window)
    ctrl.onScroll();
    // We should be at bottom so stickToBottom stays true
    expect(ctrl.stickToBottom).toBe(true);

    // Image 3 loads after 2 seconds
    ctrl.advanceTime(1200);
    ctrl.growContent(160);
    ctrl.onImageLoad();
    expect(ctrl.distFromBottom).toBe(0);
  });
});
