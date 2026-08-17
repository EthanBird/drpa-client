import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { App } from "./app/App";
import "./styles/tokens.css";
import "./styles/global.css";
import "./styles/workspace.css";
import "./styles/dashboard.css";
import "./styles/data.css";
import "./styles/local-dify.css";
import "./styles/local-dify-workflow.css";
import "./styles/secrets.css";
import "./styles/optical-transfer.css";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
