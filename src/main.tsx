import React from "react";
import ReactDOM from "react-dom/client";
import "@fontsource/be-vietnam-pro/400.css";
import "@fontsource/be-vietnam-pro/500.css";
import "@fontsource/be-vietnam-pro/600.css";
import "@fontsource/be-vietnam-pro/700.css";
import "./styles.css";
import { getCurrentWindow } from "@tauri-apps/api/window";
import App from "./App";
import { inTauri } from "./api";
import { Overlay } from "./overlay/Overlay";

// The usage mini window loads this same bundle; it never mounts <App/> (no updater check etc.).
// "#overlay" previews it in a plain browser.
const overlay = inTauri ? getCurrentWindow().label === "overlay" : location.hash === "#overlay";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    {overlay ? <Overlay /> : <App />}
  </React.StrictMode>,
);
