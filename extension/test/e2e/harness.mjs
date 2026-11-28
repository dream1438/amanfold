// Shared by the end-to-end tests: builds the server and the extension, starts
// a real amanfold-server and a server for the test pages. Test pages live on
// made-up domains; each browser is told to resolve every name to 127.0.0.1.

import { execFileSync, spawn } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const here = path.dirname(fileURLToPath(import.meta.url));
export const ext = path.resolve(here, "../..");
export const repo = path.resolve(ext, "..");

let step = 0;
export const log = (s) => console.log(`  ${++step}. ${s}`);
export const currentStep = () => step + 1;

function sh(cmd, args, opts = {}) {
  return execFileSync(cmd, args, { cwd: repo, stdio: ["ignore", "pipe", "inherit"], ...opts });
}

/** Build everything and start both servers. */
export async function start() {
  const SERVER_PORT = 18080 + Math.floor(Math.random() * 1000);
  const FIX_PORT = SERVER_PORT + 1000;
  const tmp = mkdtempSync(path.join(tmpdir(), "amanfold-e2e-"));

  if (!process.env.E2E_SKIP_BUILD) {
    sh("cargo", ["build", "-q", "-p", "amanfold-server"], { stdio: "inherit" });
    sh("node", ["scripts/wasm.mjs"], { cwd: ext, stdio: "inherit" });
  }
  sh("node", ["scripts/build.mjs", "--server", `http://127.0.0.1:${SERVER_PORT}`], { cwd: ext, stdio: "inherit" });

  const bin = path.join(repo, "target/debug/amanfold-server");
  writeFileSync(path.join(tmp, "secrets.json"), sh(bin, ["keygen"], { stdio: ["ignore", "pipe", "ignore"] }));
  const server = spawn(bin, ["serve"], {
    env: {
      ...process.env,
      AMANFOLD_SECRETS_FILE: path.join(tmp, "secrets.json"),
      AMANFOLD_DB: path.join(tmp, "amanfold.db"),
      AMANFOLD_LISTEN: `127.0.0.1:${SERVER_PORT}`,
      AMANFOLD_DEV: "1",
      AMANFOLD_NO_EMAIL_VERIFICATION: "1",
      RUST_LOG: "warn",
    },
    stdio: ["ignore", "inherit", "inherit"],
  });

  const fixtures = createServer((req, res) => {
    const name = path.basename(new URL(req.url, "http://x").pathname) || "login.html";
    try {
      const body = readFileSync(path.join(here, "fixtures", name), "utf8").replaceAll("__PORT__", String(FIX_PORT));
      res.writeHead(200, { "Content-Type": "text/html; charset=utf-8" });
      res.end(body);
    } catch {
      res.writeHead(404);
      res.end();
    }
  });
  await new Promise((r) => fixtures.listen(FIX_PORT, "127.0.0.1", r));

  for (let i = 0; ; i++) {
    try {
      const r = await fetch(`http://127.0.0.1:${SERVER_PORT}/v1/server`);
      if (r.ok) break;
    } catch {}
    if (i > 100) throw new Error("server did not start");
    await new Promise((r) => setTimeout(r, 100));
  }

  return {
    tmp,
    url: (host, page) => `http://${host}:${FIX_PORT}/${page}`,
    stop() {
      server.kill();
      fixtures.close();
      rmSync(tmp, { recursive: true, force: true });
    },
  };
}
