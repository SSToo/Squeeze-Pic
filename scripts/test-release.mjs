import { test } from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, join } from "node:path";
import { assetNames, checkVersion, platforms, prepareAssets, publishRelease } from "./release.mjs";

function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), "squeeze-pic-release-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  mkdirSync(join(root, "src-tauri"));
  writeFileSync(join(root, "package.json"), JSON.stringify({ version: "0.2.2" }));
  writeFileSync(join(root, "package-lock.json"), JSON.stringify({ version: "0.2.2", packages: { "": { version: "0.2.2" } } }));
  writeFileSync(join(root, "src-tauri/tauri.conf.json"), JSON.stringify({ version: "0.2.2" }));
  writeFileSync(join(root, "Cargo.toml"), '[workspace.package]\nversion = "0.2.2"\n');
  writeFileSync(join(root, "Cargo.lock"), ["core", "cli", "desktop"].map((name) => `[[package]]\nname = "squeeze-pic-${name}"\nversion = "0.2.2"\n`).join("\n"));
  const artifacts = join(root, "artifacts");
  for (const platform of platforms) {
    const dir = join(artifacts, `squeeze-pic-${platform.target}`);
    mkdirSync(dir, { recursive: true });
    const names = assetNames("0.2.2", platform);
    writeFileSync(join(dir, names[0]), `installer-${platform.target}`);
    writeFileSync(join(dir, names[1]), "license archive fixture");
    writeFileSync(join(dir, names[2]), JSON.stringify({ version: "0.2.2", target: platform.target, commit: process.env.GITHUB_SHA || null }));
    writeFileSync(join(dir, "SHA256SUMS.txt"), [...names].sort().map((name) => `${sha(join(dir, name))}  ${name}\n`).join(""));
  }
  return { root, artifacts, output: join(root, "output") };
}

function sha(path) { return createHash("sha256").update(readFileSync(path)).digest("hex"); }

test("reject mismatched tags, Cargo locks and project versions", (t) => {
  const { root } = fixture(t);
  assert.equal(checkVersion("v0.2.2", root), "0.2.2");
  assert.throws(() => checkVersion("v0.2.3", root), /must all match/);
  writeFileSync(join(root, "Cargo.lock"), readFileSync(join(root, "Cargo.lock"), "utf8").replace('version = "0.2.2"', 'version = "0.2.1"'));
  assert.throws(() => checkVersion("v0.2.2", root), /must all match/);
});

test("accept Windows CRLF checkout of Cargo manifests and locks", (t) => {
  const { root } = fixture(t);
  for (const name of ["Cargo.toml", "Cargo.lock"]) {
    const file = join(root, name);
    writeFileSync(file, readFileSync(file, "utf8").replaceAll("\n", "\r\n"));
  }
  assert.equal(checkVersion("v0.2.2", root), "0.2.2");
});

test("require three complete platforms and verify downloaded artifact bytes", (t) => {
  const { root, artifacts, output } = fixture(t);
  const file = join(artifacts, `squeeze-pic-${platforms[0].target}`, assetNames("0.2.2", platforms[0])[0]);
  writeFileSync(file, "changed installer");
  assert.throws(() => prepareAssets("v0.2.2", artifacts, output, root), /checksum mismatch/);
  rmSync(join(artifacts, `squeeze-pic-${platforms[0].target}`), { recursive: true });
  assert.throws(() => prepareAssets("v0.2.2", artifacts, output, root), /ENOENT/);
});

test("prepare unique installers, notices, provenance and combined checksums", (t) => {
  const { root, artifacts, output } = fixture(t);
  const files = prepareAssets("v0.2.2", artifacts, output, root);
  assert.equal(files.length, 10);
  assert.equal(new Set(files.map((file) => basename(file))).size, files.length);
  const expected = files.filter((file) => basename(file) !== "SHA256SUMS.txt")
    .map((file) => `${sha(file)}  ${basename(file)}\n`).join("");
  assert.equal(readFileSync(join(output, "SHA256SUMS.txt"), "utf8"), expected);
  assert.throws(() => prepareAssets("v0.2.2", artifacts, output, root), /must be empty/);
});

function releaseMock(initial = null, corruptUpload = false) {
  let release = initial;
  const commands = [];
  const run = (args) => {
    commands.push(args);
    if (args[0] === "api") return JSON.stringify(release);
    if (args[1] === "view") {
      if (!release) { const error = new Error("not found"); error.stderr = "release not found"; throw error; }
      return JSON.stringify({ apiUrl: "https://api.github.com/repos/test/repo/releases/1" });
    }
    if (args[1] === "create") release = { draft: true, assets: [] };
    if (args[1] === "upload") for (const path of args.slice(3, args.indexOf("--repo"))) {
      release.assets.push({ name: basename(path), state: "uploaded", size: readFileSync(path).length, digest: `sha256:${corruptUpload ? "wrong" : sha(path)}` });
    }
    if (args[1] === "edit") release.draft = false;
    return "";
  };
  return { run, commands };
}

test("publish only after every uploaded asset is verified; same bytes are idempotent", (t) => {
  const { root, artifacts, output } = fixture(t);
  const files = prepareAssets("v0.2.2", artifacts, output, root);
  const mock = releaseMock();
  publishRelease("v0.2.2", "test/repo", files, mock.run);
  assert.equal(mock.commands.filter((args) => args[1] === "edit").length, 1);
  const count = mock.commands.length;
  publishRelease("v0.2.2", "test/repo", files, mock.run);
  assert.ok(mock.commands.slice(count).every((args) => !["create", "upload", "edit"].includes(args[1])));
});

test("never overwrite different existing assets or publish a failed upload", (t) => {
  const { root, artifacts, output } = fixture(t);
  const files = prepareAssets("v0.2.2", artifacts, output, root);
  const existing = releaseMock({ draft: false, assets: [{ name: basename(files[0]), digest: "sha256:wrong" }] });
  assert.throws(() => publishRelease("v0.2.2", "test/repo", files, existing.run), /different bytes/);
  assert.ok(existing.commands.every((args) => !["upload", "edit"].includes(args[1])));
  const corrupt = releaseMock(null, true);
  assert.throws(() => publishRelease("v0.2.2", "test/repo", files, corrupt.run), /verification failed/);
  assert.ok(corrupt.commands.every((args) => args[1] !== "edit"));
});
