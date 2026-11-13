import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { App } from "./App";
import { Quick } from "./screens/Quick";
import "./styles/global.css";

// Follow the system by default (dec:telegram-port). Telegram ships both themes and
// the night palette is the better-designed one, but it is not the default.
const media = window.matchMedia("(prefers-color-scheme: dark)");
const applyTheme = (dark: boolean) =>
	document.documentElement.classList.toggle("dark", dark);
applyTheme(media.matches);
media.addEventListener("change", (e) => applyTheme(e.matches));

const root = document.getElementById("root");
if (!root) throw new Error("#root missing");

createRoot(root).render(
	<StrictMode>
		{/* The quick-access window loads the same bundle at #quick. */}
		{window.location.hash === "#quick" ? <Quick /> : <App />}
	</StrictMode>,
);
