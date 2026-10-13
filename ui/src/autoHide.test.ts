import assert from "node:assert/strict";
import test from "node:test";
import { nextHideTick, revealShouldRearm } from "./autoHide.ts";

test("already visible reveal still rearms", () => {
  assert.equal(revealShouldRearm(true, false), true);
  assert.equal(revealShouldRearm(true, true), false);
  assert.equal(revealShouldRearm(false, false), false);
});

test("pointer over the island does not stop the countdown for good", () => {
  const hovering = nextHideTick({
    autoHide: true,
    hidden: false,
    hover: true,
    remind: false,
    hint: false,
    login: false,
    stowing: false,
    idleMs: 3000,
  });
  assert.equal(hovering.kind, "wait");
  if (hovering.kind !== "wait") return;
  assert.equal(hovering.resetIdle, true);

  const left = nextHideTick({
    autoHide: true,
    hidden: false,
    hover: false,
    remind: false,
    hint: false,
    login: false,
    stowing: false,
    idleMs: 0,
  });
  assert.equal(left.kind, "wait");
  if (left.kind !== "wait") return;
  assert.equal(left.resetIdle, false);
  assert.equal(left.ms, 3000);

  const due = nextHideTick({
    autoHide: true,
    hidden: false,
    hover: false,
    remind: false,
    hint: false,
    login: false,
    stowing: false,
    idleMs: 3000,
  });
  assert.equal(due.kind, "hide");
});

test("hint login and remind keep polling", () => {
  for (const key of ["hint", "login", "remind"] as const) {
    const tick = nextHideTick({
      autoHide: true,
      hidden: false,
      hover: false,
      remind: key === "remind",
      hint: key === "hint",
      login: key === "login",
      stowing: false,
      idleMs: 3000,
    });
    assert.equal(tick.kind, "wait");
    if (tick.kind === "wait") assert.equal(tick.resetIdle, true);
  }
});

test("hidden or switched off stops", () => {
  assert.equal(nextHideTick({
    autoHide: false,
    hidden: false,
    hover: false,
    remind: false,
    hint: false,
    login: false,
    stowing: false,
    idleMs: 3000,
  }).kind, "stop");
  assert.equal(nextHideTick({
    autoHide: true,
    hidden: true,
    hover: false,
    remind: false,
    hint: false,
    login: false,
    stowing: false,
    idleMs: 3000,
  }).kind, "stop");
});
