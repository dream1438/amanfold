// Bundle the extension for Chrome/Edge (dist/chrome) and Firefox
// (dist/firefox), and zip each for the stores.
//
//   node scripts/build.mjs [--server https://api.example.com]
//
// The default server is http://127.0.0.1:8080 (a local amanfold-server).
import { build } from "esbuild";
import { cpSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";

const here = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const pkg = JSON.parse(readFileSync(path.join(here, "package.json"), "utf8"));
const arg = (name, dflt) => {
  const i = process.argv.indexOf(name);
  return i > 0 ? process.argv[i + 1] : dflt;
};
const server = arg("--server", "http://127.0.0.1:8080");
const zip = process.argv.includes("--zip");

if (!existsSync(path.join(here, "pkg/amanfold_wasm_bg.wasm"))) {
  console.error("pkg/ is missing: run `npm run wasm` first.");
  process.exit(1);
}

const manifest = (target) => {
  const m = {
    manifest_version: 3,
    name: "__MSG_extName__",
    description: "__MSG_extDescription__",
    default_locale: "en",
    version: pkg.version,
    icons: { 16: "icons/16.png", 32: "icons/32.png", 48: "icons/48.png", 128: "icons/128.png" },
    action: {
      default_popup: "popup.html",
      default_title: "Amanfold",
      default_icon: { 16: "icons/16.png", 32: "icons/32.png" },
    },
    permissions: ["storage", "alarms", "webNavigation", "clipboardWrite"],
    host_permissions: ["http://*/*", "https://*/*"],
    content_scripts: [
      {
        matches: ["http://*/*", "https://*/*"],
        js: ["content.js"],
        all_frames: true,
        run_at: "document_idle",
      },
    ],
    commands: {
      "fill-login": {
        suggested_key: { default: "Ctrl+Shift+L", mac: "Command+Shift+L" },
        description: "__MSG_cmdFill__",
      },
    },
    content_security_policy: {
      extension_pages: "script-src 'self' 'wasm-unsafe-eval'; object-src 'none'; base-uri 'none'",
    },
  };
  if (target === "chrome") {
    m.background = { service_worker: "background.js" };
    m.minimum_chrome_version = "116";
    m.version_name = `${pkg.version} beta`;
  } else {
    m.background = { scripts: ["background.js"] };
    m.browser_specific_settings = {
      gecko: {
        id: "extension@amanfold.com",
        strict_min_version: "128.0",
        data_collection_permissions: { required: ["none"] },
      },
    };
  }
  return m;
};

for (const target of ["chrome", "firefox"]) {
  const out = path.join(here, "dist", target);
  rmSync(out, { recursive: true, force: true });
  mkdirSync(out, { recursive: true });
  await build({
    entryPoints: {
      background: path.join(here, "src/background/index.ts"),
      content: path.join(here, "src/content/index.ts"),
      popup: path.join(here, "src/popup/popup.ts"),
    },
    outdir: out,
    bundle: true,
    format: "iife",
    target: target === "chrome" ? "chrome116" : "firefox128",
    minify: false,
    sourcemap: false,
    legalComments: "none",
    logLevel: "warning",
    define: {
      __AMANFOLD_SERVER__: JSON.stringify(server),
      __AMANFOLD_VERSION__: JSON.stringify(pkg.version),
      "import.meta.url": '""',
    },
  });
  cpSync(path.join(here, "src/popup/popup.html"), path.join(out, "popup.html"));
  cpSync(path.join(here, "src/popup/popup.css"), path.join(out, "popup.css"));
  cpSync(path.join(here, "static"), out, { recursive: true });
  cpSync(path.join(here, "pkg/amanfold_wasm_bg.wasm"), path.join(out, "core.wasm"));
  cpSync(path.join(here, "../LICENSE"), path.join(out, "LICENSE"));
  writeFileSync(path.join(out, "manifest.json"), JSON.stringify(manifest(target), null, 2) + "\n");
  if (zip) {
    const file = path.join(here, "dist", `amanfold-${target}-${pkg.version}.zip`);
    rmSync(file, { force: true });
    execFileSync("zip", ["-qr", "-X", file, "."], { cwd: out });
  }
  console.log(`built dist/${target} (server ${server})`);
}
