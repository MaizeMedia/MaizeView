import { mount } from "svelte";
import "../styles/app.css";
import App from "./App.svelte";
import { appearanceSettings } from "$lib/api";
import { applyAccentPreset, DEFAULT_ACCENT } from "$lib/theme";

const target = document.getElementById("app");
if (!target) throw new Error("#app mount target missing");

applyAccentPreset(DEFAULT_ACCENT);
void appearanceSettings
  .get()
  .then((s) => applyAccentPreset(s.accent_preset))
  .catch(() => {});

const app = mount(App, { target });

export default app;
