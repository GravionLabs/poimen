import { describe, expect, it } from "vitest";
import { detectPlatform, LATEST_URL, PLATFORMS, resolveDownloads, type ReleaseAsset } from "../downloads";
import { adrHeading, adrIndex, adrStatus } from "../scripts/adr-index.mjs";

const base = "https://github.com/GravionLabs/poimen/releases/download/v1.2.3/";
const names = [
  "poimen_1.2.3_amd64.AppImage",
  "poimen_1.2.3_amd64.deb",
  "poimen-1.2.3-1.x86_64.rpm",
  "poimen_1.2.3_aarch64.dmg",
  "poimen_1.2.3_x64_en-US.msi",
  "poimen_1.2.3_x64-setup.exe",
];
const assets: ReleaseAsset[] = names.map((name) => ({ name, browser_download_url: base + name }));
const platform = (id: string) => PLATFORMS.find((p) => p.id === id)!;

describe("resolveDownloads", () => {
  it("picks the installers of each platform from the asset names", () => {
    expect(resolveDownloads(platform("linux"), assets).map((d) => d.url)).toEqual([
      base + "poimen_1.2.3_amd64.AppImage",
      base + "poimen_1.2.3_amd64.deb",
      base + "poimen-1.2.3-1.x86_64.rpm",
    ]);
    expect(resolveDownloads(platform("macos"), assets).map((d) => d.url)).toEqual([base + "poimen_1.2.3_aarch64.dmg"]);
    expect(resolveDownloads(platform("windows"), assets).map((d) => d.url)).toEqual([
      base + "poimen_1.2.3_x64-setup.exe",
      base + "poimen_1.2.3_x64_en-US.msi",
    ]);
  });

  it("falls back to the latest release page without assets (not loaded, API failure)", () => {
    const links = resolveDownloads(platform("macos"), []);
    expect(links).toEqual([{ label: ".dmg", url: LATEST_URL, direct: false }]);
  });

  it("does not offer an Intel macOS dmg as the arm64 build", () => {
    const intel = [{ name: "poimen_1.2.3_x64.dmg", browser_download_url: base + "x" }];
    expect(resolveDownloads(platform("macos"), intel)[0].direct).toBe(false);
  });
});

describe("detectPlatform", () => {
  it.each([
    ["Mozilla/5.0 (Windows NT 10.0; Win64; x64)", "windows"],
    ["Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)", "macos"],
    ["Mozilla/5.0 (X11; Linux x86_64)", "linux"],
    ["Mozilla/5.0 (Linux; Android 14)", undefined],
    ["", undefined],
  ])("%s", (ua, id) => {
    expect(detectPlatform(ua)).toBe(id);
  });
});

describe("decision record index", () => {
  const text = "# 0002: Color schemes\n\n- Status: Accepted (decided on 2026-11-01)\n";
  it("reads number, title and status", () => {
    expect(adrHeading("0002-color-schemes.md", text)).toEqual({ number: "0002", title: "Color schemes" });
    expect(adrStatus(text)).toBe("Accepted");
  });
  it("falls back to the file name and a dash without heading and status", () => {
    expect(adrHeading("0009-some-thing.md", "no heading")).toEqual({ number: "0009", title: "some thing" });
    expect(adrIndex([{ file: "0009-some-thing.md", text: "x" }])).toContain("| [0009](./0009-some-thing) | some thing | - |");
  });
});
