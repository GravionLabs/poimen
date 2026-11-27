import { defineConfig } from "vitepress";

// `/<repository>/` on GitHub Pages; the workflow sets SITE_BASE.
const base = process.env["SITE_BASE"] ?? "/";
const REPO_URL = "https://github.com/GravionLabs/poimen";

export default defineConfig({
  title: "poimen",
  description:
    "A desktop GUI to watch and steer your AI coding agent sessions with editor tabs, menus and a status sidebar.",
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
      { text: "Guide", link: "/guide/getting-started" },
      { text: "GitHub", link: REPO_URL },
    ],
    sidebar: [
      {
        text: "Guide",
        items: [
          { text: "Download", link: "/download" },
          { text: "Installation", link: "/guide/installation" },
        ],
      },
      {
        text: "Using poimen",
        items: [
          { text: "Getting started", link: "/guide/getting-started" },
          { text: "Spaces and layouts", link: "/guide/spaces" },
          { text: "Agents", link: "/guide/agents" },
          { text: "GitHub and tasks", link: "/guide/github" },
          { text: "Runs", link: "/guide/runs" },
          { text: "Objective runs", link: "/guide/objective-runs" },
          { text: "Remote machines", link: "/guide/remote-machines" },
          { text: "Console (TUI and CLI)", link: "/guide/console" },
          { text: "MCP server", link: "/guide/mcp" },
          { text: "Settings", link: "/guide/settings" },
        ],
      },
    ],
    search: { provider: "local" },
    editLink: { pattern: `${REPO_URL}/edit/main/:path`, text: "Edit this page on GitHub" },
    socialLinks: [{ icon: "github", link: REPO_URL }],
    footer: { message: "Released under the MIT license." },
  },
});
