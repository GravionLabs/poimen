<script setup lang="ts">
import { withBase } from "vitepress";

// `src` is a file in site/public/screenshots. Until a real capture replaces it, the file is a labeled placeholder.
defineProps<{ src: string; alt: string; caption?: string }>();
</script>

<template>
  <figure class="shot">
    <img :src="withBase(`/screenshots/${src}`)" :alt="alt" loading="lazy" />
    <figcaption v-if="caption">{{ caption }}</figcaption>
  </figure>
</template>
