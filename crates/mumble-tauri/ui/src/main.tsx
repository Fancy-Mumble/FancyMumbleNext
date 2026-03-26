import React from "react";
import ReactDOM from "react-dom/client";
import { BrowserRouter } from "react-router-dom";
import App from "./App";
import { isMobilePlatform } from "./utils/platform";
import "./global.css";

if (isMobilePlatform()) {
  document.documentElement.style.setProperty("--titlebar-height", "0px");
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <BrowserRouter>
      <App />
    </BrowserRouter>
  </React.StrictMode>,
);
