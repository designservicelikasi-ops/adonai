import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { ToastHost } from "./components/ui";
import "./styles.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <ToastHost>
      <App />
    </ToastHost>
  </React.StrictMode>,
);

// Fade out the splash screen once the interface is mounted.
requestAnimationFrame(() => {
  const s = document.getElementById("splash");
  if (s) {
    s.style.opacity = "0";
    setTimeout(() => s.remove(), 400);
  }
});
