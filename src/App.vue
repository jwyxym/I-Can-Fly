<script setup lang="ts">
import { RouterView } from "vue-router";
</script>

<template>
	<RouterView class = 'no-scrollbar'/>
</template>

<style>
	html,
	body {
		margin: 0;
		width: var(--width);
		height: var(--height);
	}
	body #app {
		margin: 0;
		width: var(--width);
		height: calc(var(--height) - 74px);
		transform: translateY(30px);
	}
	.no-scrollbar::-webkit-scrollbar {
		display: none;
	}
</style>
