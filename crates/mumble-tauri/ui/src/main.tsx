import React from "react";
import ReactDOM from "react-dom/client";
import { BrowserRouter } from "react-router-dom";
import App from "./App";
import { isMobile } from "./utils/platform";
import { detectBackdropFilterSupport } from "./utils/platform";
import { loadPersonalization } from "./personalizationStorage";
import { applyTheme, DEFAULT_THEME } from "./themes";
import { applyFont } from "./utils/fonts";
import "./global.css";

if (isMobile) {
  document.documentElement.style.setProperty("--titlebar-height", "0px");
}

detectBackdropFilterSupport();

loadPersonalization()
  .then((p) => {
    applyTheme(p.theme);
    applyFont(p.fontFamily);
  })
  .catch(() => applyTheme(DEFAULT_THEME));

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <BrowserRouter>
      <App />
    </BrowserRouter>
  </React.StrictMode>,
);
