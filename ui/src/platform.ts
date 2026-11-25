import { useEffect, useState } from "react";
import { api } from "./api";
import type { PlatformInfo } from "./types";

const DEFAULT_PLATFORM: PlatformInfo = {
  os: "windows",
  session: "windows",
  meterSupported: true,
  islandPositionable: true,
  autoUpdateSupported: true,
  desktopNotifySupported: false,
};

let cached: PlatformInfo | null = null;
let inflight: Promise<PlatformInfo> | null = null;

export function getPlatformInfo(): Promise<PlatformInfo> {
  if (cached) return Promise.resolve(cached);
  if (!inflight) {
    inflight = api
      .platformInfo()
      .then((info) => {
        cached = info;
        return info;
      })
      .catch(() => DEFAULT_PLATFORM);
  }
  return inflight;
}

export function usePlatformInfo(): PlatformInfo {
  const [info, setInfo] = useState<PlatformInfo>(() => cached || DEFAULT_PLATFORM);
  useEffect(() => {
    let alive = true;
    void getPlatformInfo().then((next) => {
      if (alive) setInfo(next);
    });
    return () => {
      alive = false;
    };
  }, []);
  return info;
}
