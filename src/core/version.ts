// Version is injected at build time via `define` in the build script.
// Falls back to reading package.json for development mode.
declare const __APP_VERSION__: string | undefined

let cachedVersion: string | null = null

export function getVersion(): string {
  if (cachedVersion) return cachedVersion

  // Use build-time injected version if available
  if (typeof __APP_VERSION__ !== "undefined") {
    cachedVersion = __APP_VERSION__
    return cachedVersion
  }

  // Fallback: read package.json (development mode)
  try {
    const fs = require("fs")
    const path = require("path")
    const pkgPath = path.join(import.meta.dir, "..", "..", "package.json")
    const pkg = JSON.parse(fs.readFileSync(pkgPath, "utf-8"))
    cachedVersion = pkg.version ?? "0.0.0"
  } catch {
    cachedVersion = "0.0.0"
  }

  return cachedVersion
}
