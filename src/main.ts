import { bootstrapApplication } from "@angular/platform-browser";
import { provideHxAppInfo } from "@gravionlabs/helix-desktop/tauri";
import { App } from "./app/app";
import { appConfig } from "./app/app.config";
import { readAppInfo } from "./app/core/app-info";
import { TauriBridge } from "./app/core/tauri";

readAppInfo(new TauriBridge())
  .then((info) =>
    bootstrapApplication(App, {
      ...appConfig,
      providers: [...appConfig.providers, provideHxAppInfo(info)],
    }),
  )
  .catch((err) => console.error(err));
