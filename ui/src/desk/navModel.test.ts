import assert from "node:assert/strict";
import test from "node:test";
import {
  CORE_NAV_IDS,
  computeDragTargetIndex,
  normalizeNavPins,
  pinToolNav,
  reorderVisibleNav,
  resolveVisibleNavIds,
  subPageFromToolNavId,
  toolNavIdFromSubPage,
  unpinToolNav,
} from "./navModel.ts";

test("default order returns all core pages without duplicates", () => {
  const visible = resolveVisibleNavIds(undefined, undefined, []);
  assert.deepEqual(visible, [...CORE_NAV_IDS]);
});

test("pinToolNav inserts right after toolbox and keeps consecutive pins grouped", () => {
  const step1 = pinToolNav([], [], "tool:share");
  assert.deepEqual(step1.navPins, ["tool:share"]);
  assert.deepEqual(step1.navOrder, [
    "home",
    "schedule",
    "bills",
    "toolbox",
    "tool:share",
    "doors",
    "settings",
    "about",
  ]);

  const step2 = pinToolNav(step1.navOrder, step1.navPins, "tool:fetch");
  assert.deepEqual(step2.navPins, ["tool:share", "tool:fetch"]);
  assert.deepEqual(step2.navOrder, [
    "home",
    "schedule",
    "bills",
    "toolbox",
    "tool:share",
    "tool:fetch",
    "doors",
    "settings",
    "about",
  ]);

  // Duplicate pin is a no-op
  const dup = pinToolNav(step2.navOrder, step2.navPins, "tool:share");
  assert.deepEqual(dup, step2);
});

test("unpinToolNav removes shortcut from both pins and order", () => {
  const pinned = pinToolNav([], [], "tool:mute");
  const removed = unpinToolNav(pinned.navOrder, pinned.navPins, "tool:mute");
  assert.deepEqual(removed.navPins, []);
  assert.deepEqual(removed.navOrder, [...CORE_NAV_IDS]);
});

test("disabled or uninstalled pinned plugins are hidden from visible nav but preserved on reorder", () => {
  const s1 = pinToolNav([], [], "tool:plugin:edu.neu.demo");
  const s2 = pinToolNav(s1.navOrder, s1.navPins, "tool:share");

  // When plugin is enabled, both appear
  const withPlugin = resolveVisibleNavIds(s2.navOrder, s2.navPins, ["edu.neu.demo"]);
  assert.ok(withPlugin.includes("tool:plugin:edu.neu.demo"));
  assert.ok(withPlugin.includes("tool:share"));

  // When plugin is disabled, visible nav hides it
  const withoutPlugin = resolveVisibleNavIds(s2.navOrder, s2.navPins, []);
  assert.ok(!withoutPlugin.includes("tool:plugin:edu.neu.demo"));
  assert.ok(withoutPlugin.includes("tool:share"));

  // Reordering while plugin is disabled preserves the hidden plugin in saved order
  const reordered = reorderVisibleNav(s2.navOrder, s2.navPins, withoutPlugin, 0, 2);
  assert.equal(reordered[0], "schedule");
  assert.equal(reordered[2], "home");
  assert.ok(reordered.includes("tool:plugin:edu.neu.demo"));

  // Re-enabling plugin restores it cleanly
  const restored = resolveVisibleNavIds(reordered, s2.navPins, ["edu.neu.demo"]);
  assert.ok(restored.includes("tool:plugin:edu.neu.demo"));
});

test("normalizeNavPins filters invalid IDs and deduplicates", () => {
  assert.deepEqual(
    normalizeNavPins(["tool:share", "tool:share", "home", "tool:unknown", "tool:plugin:", "tool:plugin:abc"]),
    ["tool:share", "tool:plugin:abc"],
  );
});

test("subPage <-> toolNavId roundtrip works for pinnable pages only", () => {
  assert.equal(toolNavIdFromSubPage("hub"), null);
  assert.equal(toolNavIdFromSubPage("plugins"), null);
  assert.equal(toolNavIdFromSubPage("campus"), "tool:campus");
  assert.equal(toolNavIdFromSubPage("plugin:foo"), "tool:plugin:foo");
  assert.equal(subPageFromToolNavId("tool:campus"), "campus");
  assert.equal(subPageFromToolNavId("tool:plugin:foo"), "plugin:foo");
  assert.equal(subPageFromToolNavId("home"), null);
});

test("computeDragTargetIndex shifts cleanly across static item centers", () => {
  const centers = [20, 64, 108, 152];
  assert.equal(computeDragTargetIndex(centers, 1, 0), 1);
  assert.equal(computeDragTargetIndex(centers, 1, 43), 1);
  assert.equal(computeDragTargetIndex(centers, 1, 45), 2);
  assert.equal(computeDragTargetIndex(centers, 1, 120), 3);
  assert.equal(computeDragTargetIndex(centers, 1, -43), 1);
  assert.equal(computeDragTargetIndex(centers, 1, -45), 0);
  assert.equal(computeDragTargetIndex(centers, 3, -150), 0);
});
