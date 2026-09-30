import { check, type Update } from "@tauri-apps/plugin-updater";

export type UpdateLook = { update: Update | null; failed: boolean };

export async function lookForUpdate(): Promise<UpdateLook> {
  try {
    const update = await check({ timeout: 20000 });
    return { update, failed: false };
  } catch {
    return { update: null, failed: true };
  }
}
