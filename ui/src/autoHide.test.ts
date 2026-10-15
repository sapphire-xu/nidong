import assert from "node:assert/strict";
import test from "node:test";
import { HIDE_AFTER_MS, STOW_MS, shouldResetIdle, stepHide, type HideView } from "./autoHide.ts";

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
    idleMs: HIDE_AFTER_MS,
    stowMs: 0,
    ...patch,
  };
}

test("settings not ready leaves the window alone", () => {
  assert.equal(stepHide(view({ ready: false, phase: "hidden", windowVisible: true })).kind, "wait");
});

test("shown island hides only after the idle wait", () => {
  assert.deepEqual(stepHide(view({ idleMs: HIDE_AFTER_MS - 1 })), { kind: "wait", resetIdle: false });
  assert.equal(stepHide(view({ idleMs: HIDE_AFTER_MS })).kind, "begin-stow");
});

test("pointer remind hint and login do not stop the countdown for good", () => {
  for (const key of ["hover", "remind", "hint", "login"] as const) {
    const step = stepHide(view({ [key]: true, idleMs: HIDE_AFTER_MS }));
    assert.deepEqual(step, { kind: "wait", resetIdle: true });
  }
  const after = stepHide(view({ idleMs: 0 }));
  assert.deepEqual(after, { kind: "wait", resetIdle: false });
});

test("stow cancels instead of hiding when blocked", () => {
  assert.equal(stepHide(view({ phase: "stowing", stowMs: STOW_MS, hint: true })).kind, "cancel-stow");
  assert.equal(stepHide(view({ phase: "stowing", stowMs: STOW_MS, login: true })).kind, "cancel-stow");
  assert.equal(stepHide(view({ phase: "stowing", stowMs: STOW_MS, hover: true })).kind, "cancel-stow");
  assert.equal(stepHide(view({ phase: "stowing", stowMs: 2001 })).kind, "cancel-stow");
});

test("stow finishes only after the fold", () => {
  assert.deepEqual(stepHide(view({ phase: "stowing", stowMs: STOW_MS - 1 })), { kind: "wait", resetIdle: false });
  assert.equal(stepHide(view({ phase: "stowing", stowMs: STOW_MS })).kind, "finish-hide");
  assert.deepEqual(stepHide(view({ phase: "stowing", stowMs: STOW_MS, closing: true })), { kind: "wait", resetIdle: false });
});

test("hidden island that is actually visible starts over", () => {
  assert.equal(stepHide(view({ phase: "hidden", windowVisible: true })).kind, "sync-shown");
  assert.deepEqual(stepHide(view({ phase: "hidden", windowVisible: false, idleMs: HIDE_AFTER_MS })), {
    kind: "wait",
    resetIdle: false,
  });
});

test("switching auto hide off shows the island", () => {
  assert.equal(stepHide(view({ autoHide: false, phase: "hidden", windowVisible: false })).kind, "show");
  assert.equal(stepHide(view({ autoHide: false, phase: "stowing" })).kind, "show");
  assert.deepEqual(stepHide(view({ autoHide: false, phase: "shown", windowVisible: true })), {
    kind: "wait",
    resetIdle: false,
  });
});

test("fresh settings and turning auto hide on each get a full wait", () => {
  assert.equal(shouldResetIdle(false, null, false, true), false);
  assert.equal(shouldResetIdle(false, null, true, true), true);
  assert.equal(shouldResetIdle(true, true, true, true), false);
  assert.equal(shouldResetIdle(true, false, true, true), true);
  assert.equal(shouldResetIdle(true, true, true, false), false);
});

test("fullscreen hides immediately without a fold", () => {
  assert.equal(stepHide(view({ fullscreen: true, windowVisible: true, phase: "shown" })).kind, "fullscreen-hide");
  assert.equal(stepHide(view({ fullscreen: true, windowVisible: true, phase: "stowing" })).kind, "fullscreen-hide");
  assert.deepEqual(stepHide(view({ fullscreen: true, windowVisible: false, phase: "shown" })), {
    kind: "wait",
    resetIdle: false,
  });
});
