// __APP_VERSION__ is replaced at build time by the build script's `define` option.
// If not replaced (e.g. running directly with bun in dev), it falls back to "dev".
const APP_VERSION: string = typeof __APP_VERSION__ !== "undefined" ? __APP_VERSION__ : "dev"

declare const __APP_VERSION__: string

export function getVersion(): string {
  return APP_VERSION
}
