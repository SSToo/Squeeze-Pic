import { execFileSync } from "node:child_process";
import { cpSync, mkdirSync, existsSync } from "node:fs";
import { resolve } from "node:path";
const target =
  process.env.CARGO_BUILD_TARGET ||
  execFileSync("rustc", ["--print", "host-tuple"], { encoding: "utf8" }).trim();
const release = process.argv.includes("--release");
execFileSync(
  "cargo",
  [
    "build",
    "-p",
    "squeeze-pic-cli",
    "--locked",
    "--target",
    target,
    ...(release ? ["--release"] : []),
  ],
  {
    stdio: "inherit",
    env: {
      ...process.env,
      ...(process.platform === "darwin"
        ? { MACOSX_DEPLOYMENT_TARGET: "13.0" }
        : {}),
      ...(!process.env.CMAKE && existsSync("tmp/build-tools/bin/cmake")
        ? { CMAKE: resolve("tmp/build-tools/bin/cmake") }
        : {}),
    },
  },
);
const ext = target.includes("windows") ? ".exe" : "";
mkdirSync("src-tauri/binaries", { recursive: true });
cpSync(
  resolve("target", target, release ? "release" : "debug", `squeeze-pic${ext}`),
  resolve("src-tauri/binaries", `squeeze-pic-worker-${target}${ext}`),
);
