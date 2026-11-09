import React from "react";
import ReactDOM from "react-dom/client";

import App from "./App";
import "./index.css";

async function boot() {
  // Opened in a plain browser (no Tauri): use the in-memory mock backend so
  // the UI can be developed and inspected without the desktop shell.
  if (import.meta.env.DEV && !("__TAURI_INTERNALS__" in window)) {
    const { installDevMock } = await import("./lib/devMock");
    installDevMock();
  }
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>,
  );
}

void boot();
