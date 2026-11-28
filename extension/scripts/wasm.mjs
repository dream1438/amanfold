// Build the security core for the browser: extension/pkg/amanfold_wasm.{js,d.ts,_bg.wasm}.
// Needs the wasm32-unknown-unknown Rust target and wasm-bindgen-cli 0.2.130.
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const run = (cmd, args) => execFileSync(cmd, args, { cwd: root, stdio: "inherit" });
run("cargo", ["build", "-p", "amanfold-wasm", "--target", "wasm32-unknown-unknown", "--release"]);
run("wasm-bindgen", [
  "--target", "web",
  "--out-dir", "extension/pkg",
  "target/wasm32-unknown-unknown/release/amanfold_wasm.wasm",
]);
