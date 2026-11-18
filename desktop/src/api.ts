// Bridge to the native side. Every network call goes through Rust (certificate pinning).
import { invoke } from "@tauri-apps/api/core";

/* eslint-disable @typescript-eslint/no-explicit-any */
export type Json = any;

export type ApiErrorKind = "unauthorized" | "transport" | "not_configured" | "refused";

export class ApiError extends Error {
  kind: ApiErrorKind;
  constructor(message: string) {
    super(message);
    this.kind = message.startsWith("UNAUTHORIZED")
      ? "unauthorized"
      : message.startsWith("TRANSPORT")
        ? "transport"
        : message.startsWith("NOT_CONFIGURED")
          ? "not_configured"
          : "refused";
  }
}

// Visual-QA preview (build with VITE_AEGIS_PREVIEW=1): reads a capture of real engine
// responses from /preview-fixtures.json and never sends anything. Never used in the product.
const PREVIEW = import.meta.env.VITE_AEGIS_PREVIEW === "1";
let fixtures: Record<string, Json> | null = null;
async function preview<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  fixtures ??= await (await fetch("/preview-fixtures.json")).json();
  const f = fixtures!;
  if (cmd === "profile_load") return { url: "https://127.0.0.1:7879", fingerprint: "aperçu", has_token: true, local: true } as T;
  if (cmd === "api_get") return f[String(args?.path)] as T;
  if (cmd === "local_engine_status") return { installed: true, running: true, data_dir: "(aperçu)", detail: "" } as T;
  if (cmd === "api_command") return "aperçu : aucune commande envoyée" as T;
  return null as T;
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (PREVIEW) return preview<T>(cmd, args);
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    throw new ApiError(String(e));
  }
}

export interface ProfileView { url: string; fingerprint: string; has_token: boolean; local: boolean }
export interface LocalStatus { installed: boolean; running: boolean; data_dir: string; detail: string }

export const api = {
  profileLoad: () => call<ProfileView | null>("profile_load"),
  profileSave: (url: string, token: string, fingerprint: string) => call<Json>("profile_save", { url, token, fingerprint }),
  profileForget: () => call<void>("profile_forget"),
  probe: (url: string) => call<string>("fingerprint_probe", { url }),
  get: (path: string) => call<Json>("api_get", { path }),
  view: (section: string) => call<Json>("api_get", { path: `/api/v1/view/${section}` }),
  summary: () => call<Json>("api_get", { path: "/api/v1/summary" }),
  command: (type: string, args?: Json) => call<string>("api_command", { command: args === undefined ? { type } : { type, args } }),
  localStatus: () => call<LocalStatus>("local_engine_status"),
  localStart: () => call<Json>("local_engine_start"),
};
