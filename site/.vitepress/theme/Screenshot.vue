<script setup lang="ts">
import { withBase } from "vitepress";

// `src` is a file in site/public/screenshots (a labeled placeholder until a real capture exists).
// `light` is an optional light-theme variant: shown instead of `src` when the site is in light mode.
defineProps<{ src: string; alt: string; caption?: string; light?: string }>();
</script>

<template>
  <figure class="shot">
    <img :class="{ 'shot-dark': light }" :src="withBase(`/screenshots/${src}`)" :alt="alt" loading="lazy" />
    <img v-if="light" class="shot-light" :src="withBase(`/screenshots/${light}`)" :alt="alt" loading="lazy" />
    <figcaption v-if="caption">{{ caption }}</figcaption>
  </figure>
</template>
