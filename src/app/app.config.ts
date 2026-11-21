import {
  ApplicationConfig,
  inject,
  provideAppInitializer,
  provideBrowserGlobalErrorListeners,
} from "@angular/core";
import { HxMessageService, provideHxTheme } from "@gravionlabs/helix-ui";
import { PrWatch } from "./core/pr-watch";
import { POIMEN_UPDATE_BACKEND_PROVIDER } from "./core/updater";

export const appConfig: ApplicationConfig = {
  providers: [
    provideBrowserGlobalErrorListeners(),
    provideHxTheme(),
    HxMessageService,
    POIMEN_UPDATE_BACKEND_PROVIDER,
    // The checks of the pull request of a space (#995): a result is a notification and a sound.
    provideAppInitializer(() => void inject(PrWatch)),
  ],
};
