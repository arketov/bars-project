import { mount } from "svelte";
import App from "./App.svelte";
import "./theme.css";
import "./states.css";

// Окно целиком занято рабочей областью, страница не прокручивается. Но focus() и scrollIntoView
// внутри скрытых панелей всё равно сдвигают корневой прокручиваемый элемент: верхняя панель уезжает вверх.
const pin = () => {
  const root = document.scrollingElement;
  if (root && (root.scrollTop || root.scrollLeft)) root.scrollTo(0, 0);
  if (document.body.scrollTop || document.body.scrollLeft) document.body.scrollTo(0, 0);
};
window.addEventListener("scroll", pin, { passive: true });
document.addEventListener("scroll", pin, { passive: true, capture: false });

mount(App,{ target: document.getElementById("app")! });
