/**
 * Drives an `hx-select` the way a user does: through its trigger and the options of the open list.
 * `settle` renders what changed, for example `() => fixture.detectChanges()`.
 */
type Settle = () => unknown;

async function render(settle: Settle): Promise<void> {
  await settle();
  await new Promise((resolve) => setTimeout(resolve));
  await settle();
}

function trigger(select: Element): HTMLButtonElement {
  // jsdom has no scrollIntoView; the CDK listbox calls it for the active option.
  const element = select.ownerDocument.defaultView!.Element.prototype;
  element.scrollIntoView ??= () => undefined;
  const button = select.querySelector<HTMLButtonElement>(".hx-select-trigger");
  if (!button) throw new Error(`not an hx-select: <${select.localName}>`);
  return button;
}

/** The options of the open list of `select`: in the select (a dialog) or in the overlay container. */
function openList(select: Element): HTMLElement[] {
  const id = trigger(select).getAttribute("aria-controls");
  const list = id ? select.ownerDocument.getElementById(id) : null;
  return [...(list?.querySelectorAll<HTMLElement>(".hx-select-option") ?? [])];
}

/** The text of the selected option, or the placeholder. */
export function shown(select: Element): string {
  return select.querySelector(".hx-select-label")?.textContent?.trim() ?? "";
}

/** The labels of the options, in order; the list is closed again. */
export async function optionLabels(select: Element, settle: Settle): Promise<string[]> {
  trigger(select).click();
  await render(settle);
  const labels = openList(select).map((o) => o.textContent!.trim());
  trigger(select).click();
  await render(settle);
  return labels;
}

/** Opens the list and clicks the option with the label `label`. */
export async function choose(select: Element, label: string, settle: Settle): Promise<void> {
  trigger(select).click();
  await render(settle);
  const options = openList(select);
  const option = options.find((o) => o.textContent!.trim() === label);
  if (!option) {
    const known = options.map((o) => o.textContent!.trim()).join(", ");
    throw new Error(`no option "${label}" among: ${known}`);
  }
  option.click();
  await render(settle);
}
