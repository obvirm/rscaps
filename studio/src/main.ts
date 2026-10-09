import { mount } from "svelte";
import "./app.css";
import App from "./App.svelte";
import { store } from "./lib/state.svelte";

const app = mount(App, { target: document.getElementById("app")! });

if (import.meta.env.DEV) {
  (window as unknown as Record<string, unknown>).__rscaps = { store };
}

export default app;
