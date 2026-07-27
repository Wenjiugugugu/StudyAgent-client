import { createApp } from "vue";
import { createPinia } from "pinia";
import router from "./router";
import App from "./App.vue";

// Styles
import "katex/dist/katex.min.css";
import "./styles/variables.css";
import "./styles/global.css";
import "./styles/liquid-glass.css";

const app = createApp(App);

app.use(createPinia());
app.use(router);

app.mount("#app");
