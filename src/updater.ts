// In-app updates. The only network request is GitHub's latest.json for this
// repo (plus the installer when the user clicks Update). Packages are verified
// against the public key in tauri.conf.json before they are installed.
import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { check, Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { inTauri } from "./api";

export type UpdateState =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "latest" }
  | { kind: "available"; version: string; notes: string }
  | { kind: "downloading"; version: string; done: number; total: number | null }
  | { kind: "ready"; version: string }
  | { kind: "error"; message: string };

let state: UpdateState = { kind: "idle" };
let pending: Update | null = null;
let dismissed = "";
const listeners = new Set<() => void>();

function set(next: UpdateState) {
  state = next;
  listeners.forEach((l) => l());
}

const AUTO = "autoUpdate";
export function autoCheck(): boolean {
  try { return localStorage.getItem(AUTO) !== "off"; } catch { return true; }
}
export function setAutoCheck(on: boolean) {
  try { localStorage.setItem(AUTO, on ? "on" : "off"); } catch {}
}

export async function currentVersion(): Promise<string> {
  return inTauri ? getVersion() : "0.0.0-preview";
}

export async function checkForUpdate(): Promise<void> {
  if (!inTauri || state.kind === "checking" || state.kind === "downloading") return;
  set({ kind: "checking" });
  try {
    pending = await check({ timeout: 20000 });
    set(pending ? { kind: "available", version: pending.version, notes: pending.body ?? "" } : { kind: "latest" });
  } catch (e) {
    set({ kind: "error", message: String(e) });
  }
}

export async function installUpdate(): Promise<void> {
  if (!pending || state.kind === "downloading") return;
  const version = pending.version;
  let done = 0;
  let total: number | null = null;
  set({ kind: "downloading", version, done, total });
  try {
    await pending.downloadAndInstall((ev) => {
      if (ev.event === "Started") total = ev.data.contentLength ?? null;
      if (ev.event === "Progress") done += ev.data.chunkLength;
      set({ kind: "downloading", version, done, total });
    });
    // Windows quits inside downloadAndInstall to run the installer; elsewhere restart.
    set({ kind: "ready", version });
    await relaunch();
  } catch (e) {
    set({ kind: "error", message: String(e) });
  }
}

export function dismiss() {
  if (state.kind === "available") dismissed = state.version;
  listeners.forEach((l) => l());
}
export function isDismissed(s: UpdateState): boolean {
  return s.kind === "available" && s.version === dismissed;
}

export function useUpdate(): UpdateState {
  const [, force] = useState(0);
  useEffect(() => {
    const l = () => force((n) => n + 1);
    listeners.add(l);
    return () => { listeners.delete(l); };
  }, []);
  return state;
}
