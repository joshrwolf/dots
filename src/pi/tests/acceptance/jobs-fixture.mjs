// Hand-authored finite process fixture. Even a failed test cannot leave it running indefinitely.
import { writeFileSync } from "node:fs";
import { join } from "node:path";
const [mode, directory] = process.argv.slice(2);
writeFileSync(join(directory, `member-${process.pid}`), String(process.pid));
setTimeout(() => process.exit(0), 20_000);
if (mode === "ignore") process.on("SIGTERM", () => {});
if (mode === "trap") process.on("SIGTERM", () => { writeFileSync(join(directory, "trapped"), "TERM"); process.exit(0); });
if (mode === "chatty") setInterval(() => process.stdout.write("fixture output\n"), 10);
if (mode === "volume") {
  const block = Buffer.alloc(64 * 1024, "x");
  for (let i = 0; i < 160; i++) {
    if (!process.stdout.write(block)) await new Promise((resolve) => process.stdout.once("drain", resolve));
  }
  process.stdout.write("CAP_DRAIN_FINISHED\n", () => process.exit(0));
}
