import { mount } from "svelte";
import Probe from "./Probe.svelte";
import "../../theme.css";
import "../../states.css";
import "dockview/dist/styles/dockview.css";
import { invoke } from "@tauri-apps/api/core";

if ("__TAURI_INTERNALS__" in window) {
  void invoke("probe_trace", { stage: "module" });
  window.addEventListener("error", event => { void invoke("probe_finish", { result: { error: event.message } }); });
  window.addEventListener("unhandledrejection", event => { void invoke("probe_finish", { result: { error: String(event.reason) } }); });
}

mount(Probe, { target: document.getElementById("app")! });
