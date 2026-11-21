import { ApplicationConfig, provideBrowserGlobalErrorListeners } from "@angular/core";
import { HxMessageService, provideHxTheme } from "@gravionlabs/helix-ui";
import { POIMEN_UPDATE_BACKEND_PROVIDER } from "./core/updater";

export const appConfig: ApplicationConfig = {
  providers: [
    provideBrowserGlobalErrorListeners(),
    provideHxTheme(),
    HxMessageService,
    POIMEN_UPDATE_BACKEND_PROVIDER,
  ],
};
