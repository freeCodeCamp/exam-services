// Builds the npm package into js-api/pkg/
//
// Layout:
//   pkg/package.json   (hand-rolled, dual entry)
//   pkg/node/*         (wasm-pack --target nodejs, CommonJS)
//   pkg/web/*          (wasm-pack --target web, ESM + init())
//
// Usage: node js-api/build.mjs
import { execSync } from "node:child_process";
import { cpSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(fileURLToPath(import.meta.url));
const pkgDir = join(root, "pkg");
const OUT_NAME = "exam_services";

const version = readFileSync(join(root, "Cargo.toml"), "utf8").match(
  /^version = "(.*)"$/m,
)[1];

rmSync(pkgDir, { recursive: true, force: true });

for (const target of ["nodejs", "web"]) {
  const outDir = join(pkgDir, target === "nodejs" ? "node" : "web");
  execSync(
    `wasm-pack build --release --target ${target} --out-dir ${outDir} --out-name ${OUT_NAME}`,
    { cwd: root, stdio: "inherit" },
  );
  // wasm-pack metadata is superseded by the top-level package.json; the stub
  // only tells Node how to parse this directory's .js
  writeFileSync(
    join(outDir, "package.json"),
    JSON.stringify({ type: target === "nodejs" ? "commonjs" : "module" }) + "\n",
  );
  rmSync(join(outDir, ".gitignore"), { force: true });
  // wasm-pack copies the crate README into every out-dir; one at the root is enough
  rmSync(join(outDir, "README.md"), { force: true });
}

const entries = (dir) => ({
  types: `./${dir}/${OUT_NAME}.d.ts`,
  default: `./${dir}/${OUT_NAME}.js`,
});

writeFileSync(
  join(pkgDir, "package.json"),
  JSON.stringify(
    {
      name: "@freecodecamp/exam-services",
      version,
      description: "WASM bindings for the freeCodeCamp exam-services utilities",
      license: "BSD-3-Clause",
      repository: {
        type: "git",
        url: "git+https://github.com/freeCodeCamp/exam-services.git",
        directory: "js-api",
      },
      main: `node/${OUT_NAME}.js`,
      types: `node/${OUT_NAME}.d.ts`,
      exports: {
        ".": {
          node: entries("node"),
          default: entries("web"),
        },
        "./web": entries("web"),
        // direct asset access, e.g. `import wasmUrl from ".../web/exam_services_bg.wasm?url"`
        "./web/*": "./web/*",
      },
      keywords: ["freecodecamp", "exam", "wasm"],
    },
    null,
    2,
  ) + "\n",
);

cpSync(join(root, "README.md"), join(pkgDir, "README.md"));
cpSync(join(root, "..", "LICENSE"), join(pkgDir, "LICENSE"));

console.log(`\nbuilt @freecodecamp/exam-services@${version} -> ${pkgDir}`);
