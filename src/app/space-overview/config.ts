import type { TrustRequest } from "../core/space-actions";

/** The editable fields of `.poimen/config.json`; `layout` is the JSON text of the layout object. */
export interface ConfigForm {
  setup: string;
  teardown: string;
  layout: string;
}

/** The reply of `project_file`. */
export interface ProjectFile {
  root: string;
  exists: boolean;
  text: string;
}

export interface FieldError {
  field: string;
  message: string;
}

/** The reply of `project_save`. */
export interface SaveOutcome {
  saved: boolean;
  errors: FieldError[];
  trust: TrustRequest | null;
  store: string | null;
}

export const EMPTY_FORM: ConfigForm = { setup: "", teardown: "", layout: "" };

/** The form values of a file text; unreadable text gives an empty form and the reason. */
export function formOf(text: string): { form: ConfigForm; error: string | null } {
  if (text.trim() === "") return { form: { ...EMPTY_FORM }, error: null };
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch (e) {
    return { form: { ...EMPTY_FORM }, error: `The file is not valid JSON: ${String(e)}` };
  }
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    return { form: { ...EMPTY_FORM }, error: "The file is not a JSON object." };
  }
  const o = value as Record<string, unknown>;
  return {
    form: {
      setup: typeof o["setup"] === "string" ? o["setup"] : "",
      teardown: typeof o["teardown"] === "string" ? o["teardown"] : "",
      layout: o["layout"] === undefined ? "" : JSON.stringify(o["layout"], null, 2),
    },
    error: null,
  };
}

/**
 * The patch for `project_save`: every field is sent (blank removes the key). A layout that is not
 * JSON is a field error here; its structure is checked by the backend.
 */
export function patchOf(form: ConfigForm): {
  patch: Record<string, unknown>;
  errors: FieldError[];
} {
  const errors: FieldError[] = [];
  const patch: Record<string, unknown> = { setup: form.setup, teardown: form.teardown };
  if (form.layout.trim() === "") {
    patch["layout"] = null;
  } else {
    try {
      patch["layout"] = JSON.parse(form.layout);
    } catch (e) {
      errors.push({ field: "layout", message: `Not valid JSON: ${String(e)}` });
    }
  }
  return { patch, errors };
}
