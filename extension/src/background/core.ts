// The security core (Rust, compiled to WebAssembly). Every key lives inside
// its objects; this file only loads it.

import init, * as core from "../../pkg/amanfold_wasm.js";

let ready: Promise<typeof core> | undefined;

export function loadCore(): Promise<typeof core> {
  ready ??= init({ module_or_path: chrome.runtime.getURL("core.wasm") }).then(() => core);
  return ready;
}

export type Core = typeof core;
export type Account = core.Account;
export type LocalVault = core.LocalVault;
