export const CORE_NAV_IDS = [
  "home",
  "schedule",
  "bills",
  "toolbox",
  "doors",
  "settings",
  "about",
] as const;

export type CoreNavId = (typeof CORE_NAV_IDS)[number];

export const BUILTIN_TOOL_KEYS = ["campus", "share", "mute", "fetch"] as const;
export type BuiltinToolKey = (typeof BUILTIN_TOOL_KEYS)[number];

export type ToolSubPage = "hub" | BuiltinToolKey | "plugins" | `plugin:${string}`;
export type PinnableToolSubPage = BuiltinToolKey | `plugin:${string}`;
export type ToolNavId = `tool:${BuiltinToolKey}` | `tool:plugin:${string}`;
export type NavId = CoreNavId | ToolNavId;

const CORE_SET = new Set<string>(CORE_NAV_IDS);
const BUILTIN_TOOL_SET = new Set<string>(BUILTIN_TOOL_KEYS);

export function isCoreNavId(id: string): id is CoreNavId {
  return CORE_SET.has(id);
}

export function isToolNavId(id: string): id is ToolNavId {
  if (!id.startsWith("tool:")) return false;
  const rest = id.slice("tool:".length);
  if (BUILTIN_TOOL_SET.has(rest)) return true;
  if (rest.startsWith("plugin:") && rest.length > "plugin:".length) return true;
  return false;
}

export function isPinnableSubPage(sub: ToolSubPage): sub is PinnableToolSubPage {
  return BUILTIN_TOOL_SET.has(sub) || (sub.startsWith("plugin:") && sub.length > "plugin:".length);
}

export function toolNavIdFromSubPage(sub: ToolSubPage): ToolNavId | null {
  if (!isPinnableSubPage(sub)) return null;
  return `tool:${sub}` as ToolNavId;
}

export function subPageFromToolNavId(id: string): PinnableToolSubPage | null {
  if (!isToolNavId(id)) return null;
  return id.slice("tool:".length) as PinnableToolSubPage;
}

export function pluginIdFromToolNavId(id: string): string | null {
  if (!id.startsWith("tool:plugin:")) return null;
  const pluginId = id.slice("tool:plugin:".length);
  return pluginId.length > 0 ? pluginId : null;
}

export function normalizeNavPins(rawPins: readonly string[] | undefined): ToolNavId[] {
  if (!Array.isArray(rawPins)) return [];
  const out: ToolNavId[] = [];
  const seen = new Set<string>();
  for (const item of rawPins) {
    if (typeof item !== "string") continue;
    const id = item.trim();
    if (!isToolNavId(id) || seen.has(id)) continue;
    seen.add(id);
    out.push(id);
  }
  return out;
}

export function resolveVisibleNavIds(
  savedOrder: readonly string[] | undefined,
  savedPins: readonly string[] | undefined,
  availablePluginIds: readonly string[],
): NavId[] {
  const pins = normalizeNavPins(savedPins);
  const pluginSet = new Set(availablePluginIds);
  const visiblePins = pins.filter((pin) => {
    const pid = pluginIdFromToolNavId(pin);
    return pid === null || pluginSet.has(pid);
  });

  const allowed = new Set<string>([...CORE_NAV_IDS, ...visiblePins]);
  const out: NavId[] = [];
  const seen = new Set<string>();

  if (Array.isArray(savedOrder)) {
    for (const raw of savedOrder) {
      if (typeof raw !== "string") continue;
      const id = raw.trim();
      if (!allowed.has(id) || seen.has(id)) continue;
      seen.add(id);
      out.push(id as NavId);
    }
  }

  for (const id of CORE_NAV_IDS) {
    if (!seen.has(id)) {
      seen.add(id);
      out.push(id);
    }
  }

  for (const id of visiblePins) {
    if (!seen.has(id)) {
      seen.add(id);
      out.push(id);
    }
  }

  return out;
}

function materializeOrder(
  savedOrder: readonly string[] | undefined,
  pins: readonly ToolNavId[],
): NavId[] {
  const allowed = new Set<string>([...CORE_NAV_IDS, ...pins]);
  const out: NavId[] = [];
  const seen = new Set<string>();
  if (Array.isArray(savedOrder)) {
    for (const raw of savedOrder) {
      if (typeof raw !== "string") continue;
      const id = raw.trim();
      if (!allowed.has(id) || seen.has(id)) continue;
      seen.add(id);
      out.push(id as NavId);
    }
  }
  for (const id of CORE_NAV_IDS) {
    if (!seen.has(id)) {
      seen.add(id);
      out.push(id);
    }
  }
  for (const id of pins) {
    if (!seen.has(id)) {
      seen.add(id);
      out.push(id);
    }
  }
  return out;
}

export function pinToolNav(
  savedOrder: readonly string[] | undefined,
  savedPins: readonly string[] | undefined,
  toolId: ToolNavId,
): { navOrder: NavId[]; navPins: ToolNavId[] } {
  const prevPins = normalizeNavPins(savedPins);
  const baseOrder = materializeOrder(savedOrder, prevPins);
  if (prevPins.includes(toolId)) {
    return { navOrder: baseOrder, navPins: prevPins };
  }

  const nextPins = [...prevPins, toolId];
  const nextOrder = [...baseOrder];
  const toolboxIdx = nextOrder.indexOf("toolbox");
  if (toolboxIdx >= 0) {
    let insertAt = toolboxIdx + 1;
    while (insertAt < nextOrder.length && isToolNavId(nextOrder[insertAt])) {
      insertAt += 1;
    }
    nextOrder.splice(insertAt, 0, toolId);
  } else {
    nextOrder.push(toolId);
  }

  return { navOrder: nextOrder, navPins: nextPins };
}

export function unpinToolNav(
  savedOrder: readonly string[] | undefined,
  savedPins: readonly string[] | undefined,
  toolId: ToolNavId,
): { navOrder: NavId[]; navPins: ToolNavId[] } {
  const prevPins = normalizeNavPins(savedPins);
  const nextPins = prevPins.filter((p) => p !== toolId);
  const nextOrder = materializeOrder(savedOrder, nextPins).filter((id) => id !== toolId);
  return { navOrder: nextOrder, navPins: nextPins };
}

export function reorderVisibleNav(
  savedOrder: readonly string[] | undefined,
  savedPins: readonly string[] | undefined,
  visibleIds: readonly NavId[],
  fromIndex: number,
  toIndex: number,
): NavId[] {
  if (
    fromIndex < 0 ||
    toIndex < 0 ||
    fromIndex >= visibleIds.length ||
    toIndex >= visibleIds.length ||
    fromIndex === toIndex
  ) {
    return materializeOrder(savedOrder, normalizeNavPins(savedPins));
  }

  const nextVisible = [...visibleIds];
  const [moved] = nextVisible.splice(fromIndex, 1);
  nextVisible.splice(toIndex, 0, moved);

  const visibleSet = new Set<string>(nextVisible);
  const fullPrev = materializeOrder(savedOrder, normalizeNavPins(savedPins));
  const hiddenPreserved = fullPrev.filter((id) => !visibleSet.has(id));
  return [...nextVisible, ...hiddenPreserved];
}

export function computeDragTargetIndex(
  centers: readonly number[],
  fromIndex: number,
  dy: number,
): number {
  const n = centers.length;
  if (n <= 1 || fromIndex < 0 || fromIndex >= n) return fromIndex;
  const dragCenter = centers[fromIndex] + dy;
  let target = fromIndex;
  if (dy > 0) {
    while (target < n - 1 && dragCenter > centers[target + 1]) {
      target += 1;
    }
  } else if (dy < 0) {
    while (target > 0 && dragCenter < centers[target - 1]) {
      target -= 1;
    }
  }
  return target;
}
