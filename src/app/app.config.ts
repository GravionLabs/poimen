import { ApplicationConfig, provideBrowserGlobalErrorListeners } from "@angular/core";
import { provideHxTheme } from "@gravionlabs/helix-ui";

export const appConfig: ApplicationConfig = {
  providers: [provideBrowserGlobalErrorListeners(), provideHxTheme()],
};
