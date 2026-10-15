// Must be first: patches console.error before any module (incl. mediabunny /
// fframes) loads, to silence the benign mediabunny double-load warning.
import "./lib/quiet";
import { mount } from "svelte";
import "@fontsource-variable/inter";
import "@fontsource-variable/jetbrains-mono";
import "./app.css";
import App from "./App.svelte";
import { store } from "./lib/state.svelte";

const app = mount(App, { target: document.getElementById("app")! });

if (import.meta.env.DEV) {
  (window as unknown as Record<string, unknown>).__rscaps = { store };
}

export default app;
