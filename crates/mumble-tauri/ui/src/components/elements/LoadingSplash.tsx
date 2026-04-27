import { useEffect, useState } from "react";
import styles from "./LoadingSplash.module.css";

/** Pool of light-hearted loading messages.  Rotates while the app
 *  initialises so the screen never looks frozen.  Add more here
 *  freely - the splash picks them at random. */
const FUNNY_MESSAGES: readonly string[] = [
  "Reticulating splines...",
  "Warming up the microphones...",
  "Tuning the squelch knob...",
  "Convincing packets to arrive in order...",
  "Asking Opus very nicely...",
  "Negotiating with TLS handshakes...",
  "Polishing your avatar...",
  "Looking for the mute button...",
  "Counting bits, then counting them again...",
  "Brewing a fresh pot of UDP...",
  "Translating from server to human...",
  "Untangling the audio cables...",
  "Checking under the rug for lost users...",
  "Petting the denoiser...",
  "Reminding the GPU it's not invited...",
  "Loading suspiciously fast...",
  "Almost there. Probably.",
];

export interface LoadingSplashProps {
  /** Override the headline.  Defaults to "Fancy Mumble". */
  title?: string;
  /** Pin a specific subtitle.  When omitted, rotates through
   *  `FUNNY_MESSAGES` every ~1.8s. */
  message?: string;
}

/** Centered loading splash with a spinner and a rotating funny line.
 *  Use as a Suspense fallback or while initial async setup runs. */
export default function LoadingSplash({ title = "Fancy Mumble", message }: LoadingSplashProps) {
  const [tick, setTick] = useState(() => Math.floor(Math.random() * FUNNY_MESSAGES.length));

  useEffect(() => {
    if (message !== undefined) return undefined;
    const id = window.setInterval(() => {
      setTick((t) => (t + 1) % FUNNY_MESSAGES.length);
    }, 1800);
    return () => window.clearInterval(id);
  }, [message]);

  const subtitle = message ?? FUNNY_MESSAGES[tick];

  return (
    <div className={styles.root} role="status" aria-live="polite">
      <div className={styles.spinner} aria-hidden="true" />
      <div className={styles.title}>{title}</div>
      <div className={styles.subtitle}>{subtitle}</div>
    </div>
  );
}

export const __TEST_FUNNY_MESSAGES = FUNNY_MESSAGES;
