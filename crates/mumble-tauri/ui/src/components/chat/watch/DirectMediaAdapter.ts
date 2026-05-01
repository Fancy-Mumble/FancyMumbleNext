import type {
  LocalPlayerEvent,
  PlayerAdapter,
  PlayerAdapterArgs,
} from "./PlayerAdapter";

/**
 * Player adapter for a direct media URL backed by an HTML5
 * `<video>` element.
 *
 * Forwards user-initiated `play`, `pause` and `seeked` events to the
 * controller via `onLocalEvent` (only when the host is driving — the
 * controller is responsible for ignoring inbound events from
 * non-hosts).
 *
 * Programmatic state changes set `suppressEvents` so they are not
 * mistaken for local-user events and bounced back over the wire.
 */
export class DirectMediaAdapter implements PlayerAdapter {
  private readonly video: HTMLVideoElement;
  private onLocalEvent?: (event: LocalPlayerEvent) => void;
  private suppressEvents = false;

  constructor(args: PlayerAdapterArgs) {
    this.onLocalEvent = args.onLocalEvent;
    this.video = document.createElement("video");
    this.video.src = args.sourceUrl;
    this.video.controls = true;
    this.video.style.width = "100%";
    this.video.style.maxHeight = "60vh";
    this.video.style.background = "#000";
    args.container.appendChild(this.video);

    this.video.addEventListener("play", this.handlePlay);
    this.video.addEventListener("pause", this.handlePause);
    this.video.addEventListener("seeked", this.handleSeeked);
    this.video.addEventListener("ended", this.handleEnded);
  }

  async play(at: number): Promise<void> {
    this.suppressEvents = true;
    if (Math.abs(this.video.currentTime - at) > 0.5) {
      this.video.currentTime = at;
    }
    try {
      await this.video.play();
    } finally {
      this.suppressEvents = false;
    }
  }

  async pause(at: number): Promise<void> {
    this.suppressEvents = true;
    this.video.pause();
    if (Math.abs(this.video.currentTime - at) > 0.5) {
      this.video.currentTime = at;
    }
    this.suppressEvents = false;
  }

  async seek(at: number): Promise<void> {
    this.suppressEvents = true;
    this.video.currentTime = at;
    this.suppressEvents = false;
  }

  currentTime(): number {
    return this.video.currentTime;
  }

  setOnLocalEvent(cb: ((event: LocalPlayerEvent) => void) | undefined): void {
    this.onLocalEvent = cb;
  }

  destroy(): void {
    this.video.removeEventListener("play", this.handlePlay);
    this.video.removeEventListener("pause", this.handlePause);
    this.video.removeEventListener("seeked", this.handleSeeked);
    this.video.removeEventListener("ended", this.handleEnded);
    this.video.pause();
    this.video.removeAttribute("src");
    this.video.load();
    this.video.remove();
  }

  private readonly handlePlay = (): void => {
    if (this.suppressEvents) return;
    this.onLocalEvent?.({ state: "playing", currentTime: this.video.currentTime });
  };

  private readonly handlePause = (): void => {
    if (this.suppressEvents) return;
    this.onLocalEvent?.({ state: "paused", currentTime: this.video.currentTime });
  };

  private readonly handleSeeked = (): void => {
    if (this.suppressEvents) return;
    const state = this.video.paused ? "paused" : "playing";
    this.onLocalEvent?.({ state, currentTime: this.video.currentTime });
  };

  private readonly handleEnded = (): void => {
    if (this.suppressEvents) return;
    this.onLocalEvent?.({ state: "ended", currentTime: this.video.currentTime });
  };
}
