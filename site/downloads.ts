/** Where the downloads of the latest release come from (GitHub REST API, no token needed). */
export const REPO = "GravionLabs/poimen";
export const RELEASES_URL = `https://github.com/${REPO}/releases`;
export const LATEST_URL = `${RELEASES_URL}/latest`;
export const LATEST_API = `https://api.github.com/repos/${REPO}/releases/latest`;

export interface ReleaseAsset {
  name: string;
  browser_download_url: string;
}

export interface Platform {
  id: "linux" | "macos" | "windows";
  label: string;
  arch: string;
  /** Installer kinds, the first is the recommended one. Tested against the asset name. */
  kinds: { label: string; pattern: RegExp }[];
}

// The names come from tauri-action: poimen_<version>_amd64.deb, poimen-<version>-1.x86_64.rpm,
// poimen_<version>_amd64.AppImage, poimen_<version>_aarch64.dmg, poimen_<version>_x64_en-US.msi,
// poimen_<version>_x64-setup.exe. Matching is by kind and architecture, never by the version.
export const PLATFORMS: Platform[] = [
  {
    id: "linux",
    label: "Linux",
    arch: "x64",
    kinds: [
      { label: ".AppImage", pattern: /\.AppImage$/i },
      { label: ".deb", pattern: /\.deb$/i },
      { label: ".rpm", pattern: /\.rpm$/i },
    ],
  },
  {
    id: "macos",
    label: "macOS",
    arch: "Apple Silicon (arm64)",
    kinds: [{ label: ".dmg", pattern: /(aarch64|arm64).*\.dmg$/i }],
  },
  {
    id: "windows",
    label: "Windows",
    arch: "x64",
    kinds: [
      { label: ".exe", pattern: /\.exe$/i },
      { label: ".msi", pattern: /\.msi$/i },
    ],
  },
];

export interface ResolvedDownload {
  label: string;
  /** The asset of the latest release, else the page of the latest release (always valid). */
  url: string;
  /** False while the asset is not known (not loaded yet, or the release has none of this kind). */
  direct: boolean;
}

/** One link per installer kind of a platform; a kind without asset points to the latest release page. */
export function resolveDownloads(platform: Platform, assets: ReleaseAsset[]): ResolvedDownload[] {
  return platform.kinds.map((kind) => {
    const asset = assets.find((a) => kind.pattern.test(a.name));
    return asset
      ? { label: kind.label, url: asset.browser_download_url, direct: true }
      : { label: kind.label, url: LATEST_URL, direct: false };
  });
}

/** The platform of a browser, for highlighting; `undefined` when it is none of the three. */
export function detectPlatform(userAgent: string): Platform["id"] | undefined {
  if (/Windows/i.test(userAgent)) return "windows";
  if (/Mac OS X|Macintosh/i.test(userAgent)) return "macos";
  if (/Linux|X11/i.test(userAgent) && !/Android/i.test(userAgent)) return "linux";
  return undefined;
}
