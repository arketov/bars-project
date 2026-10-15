import { mount } from "svelte";
import App from "./App.svelte";
import "./theme.css";
import "./states.css";

mount(App, { target: document.getElementById("app")! });
