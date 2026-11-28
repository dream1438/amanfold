// Client for the Amanfold sync server (docs/SERVER.md). JSON over HTTPS,
// byte strings as unpadded base64url. The server sees ciphertext only.

export class ApiError extends Error {
  constructor(
    readonly status: number,
    readonly kind: string,
    message: string,
    readonly seq?: number,
  ) {
    super(`${kind}: ${message}`);
    this.name = "Server";
  }
}

/** HTTPS everywhere except this computer (development, self-hosting tests). */
export function checkServerUrl(raw: string): string {
  let u: URL;
  try {
    u = new URL(raw.trim());
  } catch {
    throw Object.assign(new Error("server address"), { name: "BadServer" });
  }
  const local = ["localhost", "127.0.0.1", "[::1]"].includes(u.hostname);
  if (u.protocol !== "https:" && !(u.protocol === "http:" && local)) {
    throw Object.assign(new Error("server must use https"), { name: "BadServer" });
  }
  if (u.username || u.password || u.search || u.hash) {
    throw Object.assign(new Error("server address"), { name: "BadServer" });
  }
  return u.origin + u.pathname.replace(/\/+$/, "");
}

export class Api {
  constructor(
    readonly base: string,
    public token?: string,
  ) {}

  async call<T = any>(method: string, path: string, body?: unknown): Promise<T> {
    const headers: Record<string, string> = {};
    if (body !== undefined) headers["Content-Type"] = "application/json";
    if (this.token) headers["Authorization"] = `Bearer ${this.token}`;
    let r: Response;
    try {
      r = await fetch(this.base + path, {
        method,
        headers,
        body: body === undefined ? undefined : JSON.stringify(body),
        credentials: "omit",
        cache: "no-store",
        redirect: "error",
      });
    } catch {
      throw new ApiError(0, "offline", "could not reach the server");
    }
    const text = await r.text();
    let json: any = {};
    if (text) {
      try {
        json = JSON.parse(text);
      } catch {
        throw new ApiError(r.status, "bad_response", "the server sent something unexpected");
      }
    }
    if (!r.ok) {
      throw new ApiError(r.status, json.error ?? "http", json.message ?? `HTTP ${r.status}`, json.seq);
    }
    return json as T;
  }

  get<T = any>(path: string) {
    return this.call<T>("GET", path);
  }
  post<T = any>(path: string, body: unknown = {}) {
    return this.call<T>("POST", path, body);
  }
  put<T = any>(path: string, body: unknown = {}) {
    return this.call<T>("PUT", path, body);
  }
  del<T = any>(path: string) {
    return this.call<T>("DELETE", path);
  }
}
