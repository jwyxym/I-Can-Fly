import { createApp } from "vue";
import App from "./App.vue";
import router from "./router";
import './script/scale';
import Varlet from '@varlet/ui';
import '@varlet/ui/es/style.mjs';

createApp(App).use(Varlet).use(router).mount("#app");
