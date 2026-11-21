import { createApplication } from "@angular/platform-browser";
import {
  createComponent,
  createEnvironmentInjector,
  provideBrowserGlobalErrorListeners,
} from "@angular/core";
import { provideHxTheme } from "@gravionlabs/helix-ui";
import { findFixture, listFixtures } from "./registry";

/** What the page tells the shot script through `window` and `document.body.dataset`. */
declare global {
  interface Window {
    __fixtures?: { id: string; title: string; viewport?: { width: number; height: number } }[];
  }
}

async function main(): Promise<void> {
  const params = new URLSearchParams(location.search);
  if (params.has("list")) {
    window.__fixtures = listFixtures();
    document.body.dataset["fixtureReady"] = "list";
    return;
  }
  const fixture = findFixture(params.get("fixture") ?? "");
  if (!fixture) {
    document.body.dataset["fixtureReady"] = "missing";
    return;
  }
  const app = await createApplication({
    providers: [provideBrowserGlobalErrorListeners(), provideHxTheme()],
  });
  const host = document.querySelector<HTMLElement>("#fixture")!;
  const ref = createComponent(fixture.component, {
    hostElement: host,
    environmentInjector: createEnvironmentInjector(fixture.providers ?? [], app.injector),
  });
  for (const [name, value] of Object.entries(fixture.inputs ?? {})) ref.setInput(name, value);
  app.attachView(ref.hostView);
  ref.changeDetectorRef.detectChanges();
  await app.whenStable();
  document.body.dataset["fixtureReady"] = fixture.id;
}

main().catch((err) => {
  console.error(err);
  document.body.dataset["fixtureReady"] = "error";
});
