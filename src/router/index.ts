import { createRouter, createWebHashHistory } from "vue-router";
import { watch } from "vue";
import { auth } from "../script/auth";

const router = createRouter({
	history: createWebHashHistory(),
	routes: [
		{
			path: "/login",
			name: "login",
			component: () => import("./login.vue"),
			meta: { public: true },
		},
		{
			path: "/sign",
			name: "sign",
			component: () => import("./sign.vue"),
			meta: { public: true },
		},
		{
			path: "/",
			name: "home",
			component: () => import("./home.vue"),
		},
		{
			path: "/settings",
			name: "settings",
			component: () => import("./settings.vue"),
		},
		{
			path: "/account/email",
			name: "change-email",
			component: () => import("./change-email.vue"),
		},
		{
			path: "/account/password",
			name: "change-password",
			component: () => import("./change-password.vue"),
		},
		{
			path: "/settings/applications",
			name: "application-settings",
			component: () => import("./applications.vue"),
		},
		{
			path: "/:pathMatch(.*)*",
			redirect: "/",
		},
	],
});

router.beforeEach(async to => {
	await auth.ready();
	const loggedIn = auth.isAuthenticated();
	if (!to.meta.public && !loggedIn) {
		return { name: "login", query: { redirect: to.fullPath } };
	}
	if (to.name === "login" && loggedIn) return { name: "home" };
});

watch(auth.session, value => {
	const current = router.currentRoute.value;
	if (!value && current.matched.length > 0 && !current.meta.public) {
		void router.replace({ name: "login", query: { redirect: current.fullPath } });
	}
});

export default router;
