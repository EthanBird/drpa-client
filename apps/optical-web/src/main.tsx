import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { WebApp } from "./WebApp";
import "../../desktop/src/styles/tokens.css";
import "../../desktop/src/styles/optical-transfer.css";
import "./web.css";

if (import.meta.env.PROD && "serviceWorker" in navigator) {
  window.addEventListener("load", () => {
    void navigator.serviceWorker.register("./sw.js");
  });
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <WebApp />
  </StrictMode>,
);
