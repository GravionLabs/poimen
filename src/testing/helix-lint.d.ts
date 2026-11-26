// @gravionlabs/helix-lint ships plain JavaScript without declarations; the lint probe spec calls `helixAllow` (#1367).
type HelixAllow = (entry: { files?: string[]; rules?: string[]; reason?: string }) => unknown;

declare module "@gravionlabs/helix-lint/eslint" {
  export const helixAllow: HelixAllow;
}

declare module "@gravionlabs/helix-lint/stylelint" {
  export const helixAllow: HelixAllow;
}
