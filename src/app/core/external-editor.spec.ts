import { Injector } from "@angular/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ExternalEditor, joinPath } from "./external-editor";
import { SpaceActions } from "./space-actions";
import { SpacesStore } from "./spaces-store";
import { TauriBridge } from "./tauri";

describe("joinPath", () => {
  it("joins with one slash", () => {
    expect(joinPath("/r/", "a/b.ts")).toBe("/r/a/b.ts");
    expect(joinPath("/r", "a.ts")).toBe("/r/a.ts");
    expect(joinPath("/r", "/x/a.ts")).toBe("/x/a.ts");
  });
});

describe("ExternalEditor", () => {
  const invoke = vi.fn();
  const notify = vi.fn();

  beforeEach(() => {
    invoke.mockReset();
    notify.mockReset();
  });

  const service = () =>
    Injector.create({
      providers: [
        { provide: TauriBridge, useValue: { invoke } },
        { provide: SpaceActions, useValue: { notify } },
        {
          provide: SpacesStore,
          useValue: { snapshot: () => ({ spaces: [{ space_id: "s1", cwd: "/r" }] }) },
        },
        { provide: ExternalEditor, useClass: ExternalEditor, deps: [] },
      ],
    }).get(ExternalEditor);

  it("opens a path at a line", async () => {
    invoke.mockResolvedValue(undefined);
    const ok = await service().open("/r/a.ts", { line: 7 });
    expect(ok).toBe(true);
    expect(invoke).toHaveBeenCalledWith("open_in_editor", {
      path: "/r/a.ts",
      line: 7,
      column: null,
    });
    expect(notify).not.toHaveBeenCalled();
  });

  it("shows the reason as a notice when the editor is not found", async () => {
    invoke.mockRejectedValue('the editor "code" was not found; set "editor" in config.json');
    const ok = await service().open("/r");
    expect(ok).toBe(false);
    expect(notify).toHaveBeenCalledWith(
      expect.stringContaining('"editor" in config.json'),
      "error",
    );
  });
});

describe("ExternalEditor entries", () => {
  const invoke = vi.fn(async () => undefined);
  const notify = vi.fn();
  const service = () =>
    Injector.create({
      providers: [
        { provide: TauriBridge, useValue: { invoke } },
        { provide: SpaceActions, useValue: { notify } },
        {
          provide: SpacesStore,
          useValue: {
            snapshot: () => ({ spaces: [{ space_id: "s1", cwd: "/work/s1" }] }),
            all: () => ({
              spaces: [
                { space_id: "s1", cwd: "/work/s1" },
                { space_id: "box::s2", cwd: "/srv/s2", machine: "box" },
              ],
            }),
          },
        },
        { provide: ExternalEditor, useClass: ExternalEditor, deps: [] },
      ],
    }).get(ExternalEditor);

  it("opens the directory of a space", () => {
    service().openSpace("s1");
    expect(invoke).toHaveBeenCalledWith("open_in_editor", {
      path: "/work/s1",
      line: null,
      column: null,
    });
  });

  it("opens the directory of a space of a machine over ssh (#1005)", () => {
    service().openSpace("box::s2");
    expect(invoke).toHaveBeenCalledWith("open_in_editor", {
      path: "/srv/s2",
      line: null,
      column: null,
      machine: "box",
    });
  });

  it("names the setting when the remote editor cannot be started", async () => {
    invoke.mockRejectedValueOnce("no editor with remote support (VS Code, Zed) found on the PATH");
    expect(await service().openRemote("box", "/srv/s2")).toBe(false);
    expect(notify).toHaveBeenCalledWith(expect.stringContaining("remote support"), "error");
  });

  it("says so when the palette command has no active tab", () => {
    service().openDir(undefined);
    expect(notify).toHaveBeenCalledWith(expect.stringContaining("no active tab"));
  });
});
