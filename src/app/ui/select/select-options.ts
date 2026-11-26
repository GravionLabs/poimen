/** An option of an `hx-select` (`[options]`). Keep the array stable: build it in a `computed`. */
export interface SelectOption {
  label: string;
  value: string;
  disabled?: boolean;
}

/** Options whose label is the value. */
export function selectOptions(values: readonly string[]): SelectOption[] {
  return values.map((value) => ({ label: value, value }));
}
