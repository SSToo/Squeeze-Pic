import { execFileSync } from "node:child_process";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { basename, join, relative, resolve } from "node:path";

const destination = resolve("third-party");
mkdirSync(destination, { recursive: true });
for (const sub of ["licenses", "sources"]) {
  rmSync(join(destination, sub), { recursive: true, force: true });
  mkdirSync(join(destination, sub), { recursive: true });
}
const notices = [];
function collect(root, id) {
  const found = [];
  function walk(directory, depth) {
    for (const item of readdirSync(directory, { withFileTypes: true })) {
      const path = join(directory, item.name);
      if (
        item.isFile() &&
        /^(licen[cs]e|copying|notice|patents)([._-]|$)|^readme\.ijg$/i.test(
          item.name,
        )
      ) {
        const target = join(destination, "licenses", id, relative(root, path));
        mkdirSync(resolve(target, ".."), { recursive: true });
        copyFileSync(path, target);
        found.push(relative(destination, target));
      } else if (
        item.isDirectory() &&
        depth < 5 &&
        /^(vendor|licenses?|third[-_]party|libwebp|mozjpeg|libjpeg-turbo|extras|libjxl|highway|brotli)$/i.test(
          item.name,
        )
      )
        walk(path, depth + 1);
    }
  }
  walk(root, 0);
  return found;
}
const metadata = JSON.parse(
  execFileSync("cargo", ["metadata", "--locked", "--format-version", "1"], {
    encoding: "utf8",
    maxBuffer: 16 * 1024 * 1024,
  }),
);
for (const pkg of metadata.packages.filter((p) => p.source)) {
  const id = `rust-${pkg.name}-${pkg.version}`;
  const root = resolve(pkg.manifest_path, "..");
  const files = collect(root, id);
  if (pkg.license_file) {
    const source = resolve(root, pkg.license_file);
    if (existsSync(source)) {
      const output = join(destination, "licenses", id, basename(source));
      mkdirSync(resolve(output, ".."), { recursive: true });
      copyFileSync(source, output);
    }
  }
  if ((pkg.license || "").includes("MPL"))
    execFileSync("tar", [
      "-czf",
      join(destination, "sources", `${id}.tar.gz`),
      "-C",
      root,
      ".",
    ]);
  notices.push({
    ecosystem: "Rust",
    name: pkg.name,
    version: pkg.version,
    license: pkg.license,
    repository: pkg.repository,
    licenseFiles: files,
  });
}
const lock = JSON.parse(readFileSync("package-lock.json", "utf8"));
for (const path of Object.keys(lock.packages).filter((p) =>
  p.startsWith("node_modules/"),
)) {
  const manifest = join(path, "package.json");
  if (!existsSync(manifest)) continue;
  const pkg = JSON.parse(readFileSync(manifest, "utf8"));
  const files = collect(
    path,
    `npm-${pkg.name.replaceAll("/", "_")}-${pkg.version}`,
  );
  notices.push({
    ecosystem: "npm",
    name: pkg.name,
    version: pkg.version,
    license: pkg.license,
    licenseFiles: files,
  });
}
for (const file of [
  "THIRD-PARTY.md",
  "licenses/Squeeze-PDF-LICENSE.txt",
  "licenses/Squeeze-PDF-NOTICE.txt",
])
  copyFileSync(file, join(destination, basename(file)));
writeFileSync(
  join(destination, "inventory.json"),
  JSON.stringify(notices, null, 2),
);
writeFileSync(
  join(destination, "NOTICES.txt"),
  notices
    .map(
      (p) =>
        `${p.ecosystem}: ${p.name} ${p.version}\nLicense: ${p.license || "See package source"}\n${p.licenseFiles.join("\n")}`,
    )
    .join("\n\n"),
);
console.log(`Prepared notices for ${notices.length} packages.`);
