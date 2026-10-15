import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  copyFileSync, lstatSync, mkdirSync, readFileSync, readdirSync, writeFileSync,
} from "node:fs";
import { basename, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const platforms = [
  { target: "x86_64-pc-windows-msvc", label: "windows-x64", bundle: "nsis", suffix: "x64-setup.exe", ext: "exe" },
  { target: "aarch64-apple-darwin", label: "macos-arm64", bundle: "dmg", suffix: "aarch64.dmg", ext: "dmg" },
  { target: "x86_64-apple-darwin", label: "macos-x64", bundle: "dmg", suffix: "x86_64.dmg", ext: "dmg" },
];

export function checkVersion(tag = "", root = resolve(".")) {
  const json = (path) => JSON.parse(readFileSync(join(root, path), "utf8"));
  const version = json("package.json").version;
  const cargo = readFileSync(join(root, "Cargo.toml"), "utf8")
    .match(/\[workspace\.package\]([^]*?)(?=\n\[|$)/)?.[1]
    .match(/^version\s*=\s*"([^"]+)"/m)?.[1];
  const lock = json("package-lock.json");
  const cargoLock = readFileSync(join(root, "Cargo.lock"), "utf8");
  const crates = ["squeeze-pic-core", "squeeze-pic-cli", "squeeze-pic-desktop"];
  if (
    !/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(version) ||
    (tag && tag !== `v${version}`) || cargo !== version ||
    json("src-tauri/tauri.conf.json").version !== version ||
    lock.version !== version || lock.packages[""].version !== version ||
    !crates.every((name) => cargoLock.includes(`name = "${name}"\nversion = "${version}"`))
  ) throw new Error(`Project versions and release tag must all match v${version}.`);
  return version;
}

function digest(path) {
  const stat = lstatSync(path);
  if (!stat.isFile() || !stat.size) throw new Error(`Missing or empty asset: ${path}`);
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

function emptyDirectory(path) {
  mkdirSync(path, { recursive: true });
  if (readdirSync(path).length) throw new Error(`Output directory must be empty: ${path}`);
}

export function assetNames(version, platform) {
  const prefix = `Squeeze-Pic_${version}_${platform.label}`;
  return [`${prefix}.${platform.ext}`, `${prefix}-notices.tar.gz`, `${prefix}-build.json`];
}

function checksums(directory, names) {
  return [...names].sort().map((name) => `${digest(join(directory, name))}  ${name}\n`).join("");
}

export function stagePlatform(target, output, root = resolve(".")) {
  const platform = platforms.find((item) => item.target === target);
  if (!platform) throw new Error(`Unsupported target: ${target}`);
  const version = checkVersion("", root);
  const bundle = join(root, "target", target, "release/bundle");
  const mac = platform.ext === "dmg";
  const source = join(bundle, platform.bundle, mac
    ? `Squeeze-Pic-${version}-${platform.suffix}`
    : `Squeeze Pic_${version}_${platform.suffix}`);
  digest(source);
  const worker = mac
    ? join(bundle, "macos/Squeeze Pic.app/Contents/MacOS/squeeze-pic-worker")
    : join(root, "src-tauri/binaries", `squeeze-pic-worker-${target}.exe`);
  const workerVersion = execFileSync(worker, ["--version"], { encoding: "utf8" }).trim();
  if (workerVersion !== `squeeze-pic ${version}`) throw new Error(`Wrong bundled worker: ${workerVersion}`);
  const inventory = JSON.parse(readFileSync(join(root, "third-party/inventory.json"), "utf8"));
  if (!Array.isArray(inventory) || !inventory.length) throw new Error("Missing third-party notices.");
  emptyDirectory(output);
  const names = assetNames(version, platform);
  copyFileSync(source, join(output, names[0]));
  execFileSync("tar", ["-czf", resolve(output, names[1]), "-C", join(root, "third-party"), "."]);
  writeFileSync(join(output, names[2]), JSON.stringify({
    version, target, commit: process.env.GITHUB_SHA || null,
    repository: process.env.GITHUB_REPOSITORY || null,
    worker_sha256: digest(worker),
    signing: mac ? "ad-hoc; not notarized" : "unsigned",
  }, null, 2) + "\n");
  writeFileSync(join(output, "SHA256SUMS.txt"), checksums(output, names));
}

export function prepareAssets(tag, artifacts, output, root = resolve(".")) {
  const version = checkVersion(tag, root);
  const assets = [];
  for (const platform of platforms) {
    const directory = join(artifacts, `squeeze-pic-${platform.target}`);
    const names = assetNames(version, platform);
    if (readFileSync(join(directory, "SHA256SUMS.txt"), "utf8") !== checksums(directory, names))
      throw new Error(`Artifact checksum mismatch: ${platform.target}`);
    const info = JSON.parse(readFileSync(join(directory, names[2]), "utf8"));
    if (info.version !== version || info.target !== platform.target ||
      (process.env.GITHUB_SHA && info.commit !== process.env.GITHUB_SHA))
      throw new Error(`Artifact build mismatch: ${platform.target}`);
    for (const name of names) assets.push({ source: join(directory, name), name });
  }
  emptyDirectory(output);
  for (const { source, name } of assets) copyFileSync(source, join(output, name));
  const names = assets.map((asset) => asset.name).sort();
  writeFileSync(join(output, "SHA256SUMS.txt"), checksums(output, names));
  return [...names, "SHA256SUMS.txt"].map((name) => join(output, name));
}

function gh(args) {
  return execFileSync("gh", args, { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
}

export function publishRelease(tag, repository, files, run = gh) {
  if (!/^[\w.-]+\/[\w.-]+$/.test(repository || "")) throw new Error("Invalid GitHub repository.");
  const readRelease = () => {
    try {
      const { apiUrl } = JSON.parse(run(["release", "view", tag, "--repo", repository, "--json", "apiUrl"]));
      return JSON.parse(run(["api", apiUrl]));
    } catch (error) {
      if (/HTTP 404|release not found/i.test(String(error.stderr))) return null;
      throw error;
    }
  };
  const expected = files.map((path) => ({ path, name: basename(path), size: lstatSync(path).size, digest: `sha256:${digest(path)}` }));
  const matches = (asset, file) => asset.state === "uploaded" && asset.size === file.size && asset.digest === file.digest;
  let release = readRelease();
  if (!release) {
    run(["release", "create", tag, "--repo", repository, "--verify-tag", "--draft", "--title", `Squeeze Pic ${tag}`, "--generate-notes", ...(tag.includes("-") ? ["--prerelease"] : [])]);
    release = readRelease();
  }
  if (!release) throw new Error("Release was not created.");
  const missing = [];
  for (const file of expected) {
    const asset = release.assets.find((item) => item.name === file.name);
    if (!asset) missing.push(file.path);
    else if (!matches(asset, file)) throw new Error(`Existing asset has different bytes: ${file.name}. Use a new version tag.`);
  }
  if (missing.length) run(["release", "upload", tag, ...missing, "--repo", repository]);
  release = readRelease();
  if (!release || !expected.every((file) => release.assets.some((asset) => asset.name === file.name && matches(asset, file))))
    throw new Error("Release upload verification failed; draft has not been published.");
  if (release.draft) run(["release", "edit", tag, "--repo", repository, "--draft=false"]);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [command, first, second, third] = process.argv.slice(2);
  if (command === "check-version") console.log(checkVersion(first));
  else if (command === "stage" && first && second) stagePlatform(first, second);
  else if (["prepare", "publish"].includes(command) && first && second && third) {
    const files = prepareAssets(first, second, third);
    if (command === "publish") publishRelease(first, process.env.GITHUB_REPOSITORY, files);
    console.log(`Prepared ${files.length} release assets.`);
  } else throw new Error("Usage: release.mjs check-version [tag] | stage target output | prepare/publish tag artifacts output");
}
