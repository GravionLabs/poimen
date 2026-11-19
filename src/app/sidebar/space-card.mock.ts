import { readFileSync } from "node:fs";
import { TestBed } from "@angular/core/testing";
import { SpaceCard } from "./space-card";

/** Specs run without the Angular resource transform: give the card its template from the file. */
export function inlineSpaceCard(): void {
  TestBed.overrideComponent(SpaceCard, {
    set: {
      templateUrl: undefined,
      styleUrl: undefined,
      template: readFileSync("src/app/sidebar/space-card.html", "utf8"),
    },
  });
}
