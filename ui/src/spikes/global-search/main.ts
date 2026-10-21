import { mount } from "svelte";
import Design from "./Design.svelte";
import "../../theme.css";
import "../../states.css";

// Только стенд дизайна. App вне Tauri использует штатный mock.ts.
mount(Design, { target: document.getElementById("app")! });
