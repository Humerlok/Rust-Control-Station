/** Entry point: mounts <App /> into #root. */
import { createRoot } from "react-dom/client";
import { App } from "./App";
import "./theme.css";
import "./app.css";

createRoot(document.getElementById("root")!).render(<App />);
