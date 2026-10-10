<script setup lang="ts">
	import { onMounted, onUnmounted, ref, watch } from "vue";
	import { applicationIconFallback, getApplicationIcon } from "../script/applicationIcons";

	const props = defineProps<{ source: string }>();
	const element = ref<HTMLImageElement>();
	const icon = ref(applicationIconFallback);
	const visible = ref(false);
	let observer: IntersectionObserver | undefined;
	let version = 0;

	watch([() => props.source, visible], async ([source, isVisible]) => {
		const requestVersion = ++version;
		icon.value = applicationIconFallback;
		if (!isVisible) return;
		const result = await getApplicationIcon(source);
		if (requestVersion === version) icon.value = result;
	});

	onMounted(() => {
		if (!element.value || typeof IntersectionObserver === 'undefined') {
			visible.value = true;
			return;
		}
		observer = new IntersectionObserver(entries => {
			if (!entries.some(entry => entry.isIntersecting)) return;
			visible.value = true;
			observer?.disconnect();
		});
		observer.observe(element.value);
	});
	onUnmounted(() => {
		version++;
		observer?.disconnect();
	});
</script>

<template>
	<img ref="element" class="application-icon" :src="icon" alt="" loading="lazy"
		@error="icon = applicationIconFallback" />
</template>

<style scoped>
	.application-icon {
		width: 40px;
		height: 40px;
		margin-right: 12px;
		border-radius: 8px;
		object-fit: contain;
		flex-shrink: 0;
	}
</style>
