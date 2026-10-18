import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { HIDE_AFTER_MS, HIDE_PULSE_EVENT, STOW_MS, stepHide, type HideView } from "./autoHide.ts";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");

function view(patch: Partial<HideView>): HideView {
  return {
    ready: true,
    autoHide: true,
    phase: "shown",
    windowVisible: true,
    hover: false,
    remind: false,
    hint: false,
    login: false,
    fullscreen: false,
    closing: false,
    idleMs: 0,
    stowMs: 0,
    ...patch,
  };
}

test("boot timetable stays up only until a host pulse, without toggling auto hide", () => {
  assert.equal(HIDE_PULSE_EVENT, "island-hide-pulse");
  // 设置还没到，脉冲也不许动窗口。
  assert.equal(stepHide(view({ ready: false, idleMs: HIDE_AFTER_MS })).kind, "wait");
  // 同步句还在，空闲已经够 3 秒：要开始收，不要被「同步中」按住。
  assert.equal(stepHide(view({ login: true, idleMs: HIDE_AFTER_MS })).kind, "begin-stow");
  // 定时器停了很久，课表已经画出来。下一次宿主脉冲仍要收起，不必先把自动隐藏关开一次。
  assert.equal(stepHide(view({ login: false, hint: false, idleMs: 60_000 })).kind, "begin-stow");
  assert.equal(stepHide(view({ phase: "stowing", stowMs: STOW_MS, login: false })).kind, "finish-hide");
});

test("the island subscribes to the rust hide pulse", () => {
  const app = fs.readFileSync(path.join(root, "ui/src/App.tsx"), "utf8");
  const rust = fs.readFileSync(path.join(root, "src-tauri/src/lib.rs"), "utf8");
  assert.match(app, /listen\(\s*HIDE_PULSE_EVENT/);
  assert.match(rust, new RegExp(`"${HIDE_PULSE_EVENT}"`));
  assert.match(rust, /spawn_hide_pulse\(/);
});
