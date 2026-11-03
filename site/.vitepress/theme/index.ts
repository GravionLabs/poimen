import DefaultTheme from "vitepress/theme";
import type { Theme } from "vitepress";
import Downloads from "./Downloads.vue";
import Screenshot from "./Screenshot.vue";
import "./style.css";

export default {
  extends: DefaultTheme,
  enhanceApp({ app }) {
    app.component("Downloads", Downloads);
    app.component("Screenshot", Screenshot);
  },
} satisfies Theme;
