import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import "./styles.css";
import "./connection.css";
import "./traffic.css";
import "./replay.css";
import "./mocks.css";
import "./workspace.css";
import "./settings.css";
import "./sdk.css";
import "./compare.css";
import "./ai.css";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
