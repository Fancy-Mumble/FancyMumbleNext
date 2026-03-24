import { getCurrentWindow } from "@tauri-apps/api/window";
import { isDesktopPlatform } from "../utils/platform";
import InfoFilledIcon from "../assets/icons/status/info-filled.svg?react";
import MinimizeIcon from "../assets/icons/window/minimize.svg?react";
import MaximizeIcon from "../assets/icons/window/maximize.svg?react";
import WindowCloseIcon from "../assets/icons/window/close.svg?react";
import styles from "./TitleBar.module.css";

export default function TitleBar() {
  // On mobile (Android/iOS) there is no custom title bar - the OS
  // provides its own status bar and navigation.
  if (!isDesktopPlatform()) {
    return null;
  }

  const appWindow = getCurrentWindow();

  const handleMinimize = async () => {
    await appWindow.minimize();
  };

  const handleMaximize = async () => {
    await appWindow.toggleMaximize();
  };

  const handleClose = async () => {
    await appWindow.close();
  };

  return (
    <div className={styles.titleBar} data-tauri-drag-region>
      <div className={styles.titleSection} data-tauri-drag-region>
        <div className={styles.logo}>
          <InfoFilledIcon width={20} height={20} />
        </div>
        <span className={styles.title}>Fancy Mumble</span>
      </div>

      <div className={styles.controls}>
        <button
          className={styles.controlBtn}
          onClick={handleMinimize}
          aria-label="Minimize"
        >
          <MinimizeIcon width={12} height={12} />
        </button>
        <button
          className={styles.controlBtn}
          onClick={handleMaximize}
          aria-label="Maximize"
        >
          <MaximizeIcon width={12} height={12} />
        </button>
        <button
          className={`${styles.controlBtn} ${styles.closeBtn}`}
          onClick={handleClose}
          aria-label="Close"
        >
          <WindowCloseIcon width={12} height={12} />
        </button>
      </div>
    </div>
  );
}
