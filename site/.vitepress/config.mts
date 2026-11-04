import { defineConfig } from "vitepress";

// `/<repository>/` on GitHub Pages; the workflow sets SITE_BASE.
const base = process.env["SITE_BASE"] ?? "/";
const REPO_URL = "https://github.com/GravionLabs/poimen";

export default defineConfig({
  title: "poimen",
  description:
    "A desktop GUI for herdr: watch and steer your AI coding agent sessions with editor tabs, menus and a status sidebar.",
  lang: "en",
  base,
  cleanUrls: true,
  lastUpdated: false,
  // A link that leads nowhere fails the build.
  ignoreDeadLinks: false,
  head: [["link", { rel: "icon", type: "image/png", href: `${base}logo.png` }]],
  themeConfig: {
    logo: "/logo.png",
    nav: [
      { text: "Download", link: "/download" },
      { text: "Installation", link: "/guide/installation" },
      { text: "Features", link: "/guide/features" },
      { text: "GitHub", link: REPO_URL },
    ],
    sidebar: [
      {
        text: "Guide",
        items: [
          { text: "Download", link: "/download" },
          { text: "Installation", link: "/guide/installation" },
          { text: "Features", link: "/guide/features" },
        ],
      },
    ],
    search: { provider: "local" },
    editLink: { pattern: `${REPO_URL}/edit/main/:path`, text: "Edit this page on GitHub" },
    socialLinks: [{ icon: "github", link: REPO_URL }],
    footer: { message: "Released under the MIT license." },
  },
});
