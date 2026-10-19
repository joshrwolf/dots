// A finite owner used only to exercise abrupt IPC-owner death and start races.
import { fork } from "node:child_process";
const [supervisor, directory, command, timing] = process.argv.slice(2);
const child = fork(supervisor, [], { detached: true, stdio: ["ignore", "ignore", "ignore", "ipc"], execArgv: ["--experimental-strip-types"] });
child.on("error", () => {});
process.on("error", () => {});
const die = () => process.kill(process.pid, "SIGKILL");
process.send?.({ supervisor: child.pid }, () => {
  if (timing === "before-ready") die();
});
child.on("message", (message) => {
  if (message.type !== "ready" || timing === "before-ready") return;
  if (timing !== "no-start") child.send({ type: "start", directory, cwd: directory, command, timeoutMs: 10_000 }, () => {});
  setTimeout(die, timing === "no-start" ? 0 : Number(timing));
});
setTimeout(die, 5000);
