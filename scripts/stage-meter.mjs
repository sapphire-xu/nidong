import { spawnSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

if (process.platform !== "win32") {
  console.log(`[nidong] skipping Windows nidong-meter staging on ${process.platform}`);
  process.exit(0);
}

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const bat = path.join(root, "scripts", "stage-meter.bat");
const res = spawnSync("cmd.exe", ["/c", bat], {
  cwd: root,
  stdio: "inherit",
});
process.exit(res.status ?? 1);
