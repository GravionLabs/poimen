<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import {
  detectPlatform,
  LATEST_API,
  LATEST_URL,
  PLATFORMS,
  RELEASES_URL,
  resolveDownloads,
  type ReleaseAsset,
} from "../../downloads";

const assets = ref<ReleaseAsset[]>([]);
const tag = ref("");
const mine = ref<string | undefined>();

// The installer names carry the version, so the links are resolved from the latest release when the page opens.
// Without JavaScript or when the API does not answer, every button leads to the page of the latest release.
onMounted(async () => {
  mine.value = detectPlatform(navigator.userAgent);
  try {
    const response = await fetch(LATEST_API, { headers: { Accept: "application/vnd.github+json" } });
    if (!response.ok) return;
    const release = await response.json();
    assets.value = Array.isArray(release.assets) ? release.assets : [];
    tag.value = typeof release.tag_name === "string" ? release.tag_name : "";
  } catch {
    // keep the fallback links
  }
});

const cards = computed(() =>
  PLATFORMS.map((platform) => ({ platform, links: resolveDownloads(platform, assets.value) })),
);
</script>

<template>
  <div>
    <div class="dl-grid">
      <div v-for="{ platform, links } in cards" :key="platform.id" :class="['dl-card', { mine: mine === platform.id }]">
        <h3>{{ platform.label }}</h3>
        <p>{{ platform.arch }}</p>
        <div class="dl-links">
          <a v-for="link in links" :key="link.label" :href="link.url" :class="{ fallback: !link.direct }">{{
            link.label
          }}</a>
        </div>
      </div>
    </div>
    <p class="dl-note">
      <template v-if="tag">Latest release: {{ tag }}. </template>
      <a :href="LATEST_URL">Latest release page</a> | <a :href="RELEASES_URL">All releases</a>. The installers are not
      code-signed yet, see the installation guide.
    </p>
  </div>
</template>
