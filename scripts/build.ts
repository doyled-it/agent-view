#!/usr/bin/env bun

import path from "path"
import solidPlugin from "@opentui/solid/bun-plugin"

const dir = path.resolve(import.meta.dir, "..")
process.chdir(dir)

const pkg = await Bun.file("./package.json").json()

// Inject version into source before bundling — Bun's `define` doesn't
// survive the compile step, so we write it directly into the source file.
const versionFile = path.join(dir, "src/core/version.ts")
const versionContent = `// This file is updated at build time by scripts/build.ts.
// In development, run \`bun run build\` to update the version.
export const APP_VERSION = ${JSON.stringify(pkg.version)}

export function getVersion(): string {
  return APP_VERSION
}
`
await Bun.write(versionFile, versionContent)

const result = await Bun.build({
  entrypoints: ["./src/index.ts"],
  outdir: "./dist",
  target: "bun",
  format: "esm",
  splitting: false,
  sourcemap: "external",
  minify: false,
  plugins: [solidPlugin],
  external: ["bun:sqlite", "node-pty"],
})

if (!result.success) {
  console.error("Build failed:")
  for (const log of result.logs) {
    console.error(log)
  }
  process.exit(1)
}

console.log("Build successful!")
console.log(`Output: ${result.outputs.map(o => o.path).join(", ")}`)
