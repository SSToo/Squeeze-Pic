import { execFileSync } from "node:child_process";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  symlinkSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

if (process.platform !== "darwin") throw new Error("Run on macOS.");
const target = process.env.CARGO_BUILD_TARGET;
const root = resolve("target", ...(target ? [target] : []), "release/bundle");
const app = join(root, "macos/Squeeze Pic.app");
if (!existsSync(join(app, "Contents/Resources/third-party/inventory.json")))
  throw new Error("Build the app with bundled notices first.");
const triple =
  target ||
  execFileSync("rustc", ["--print", "host-tuple"], { encoding: "utf8" }).trim();
const version = JSON.parse(readFileSync("package.json", "utf8")).version;
mkdirSync(join(root, "dmg"), { recursive: true });
const output = join(
  root,
  "dmg",
  `Squeeze-Pic-${version}-${triple.split("-")[0]}.dmg`,
);
const stage = mkdtempSync(join(tmpdir(), "squeeze-pic-dmg-"));
try {
  execFileSync("ditto", [app, join(stage, "Squeeze Pic.app")]);
  copyFileSync("THIRD-PARTY.md", join(stage, "Third-party notices.md"));
  symlinkSync("/Applications", join(stage, "Applications"));
  execFileSync(
    "hdiutil",
    [
      "create",
      "-volname",
      "Squeeze Pic",
      "-srcfolder",
      stage,
      "-ov",
      "-format",
      "UDZO",
      output,
    ],
    { stdio: "inherit" },
  );
  execFileSync("hdiutil", ["verify", output], { stdio: "inherit" });
  console.log(output);
} finally {
  rmSync(stage, { recursive: true, force: true });
}
