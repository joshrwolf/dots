import {appendFileSync} from "node:fs";
import type {ExtensionAPI} from "@earendil-works/pi-coding-agent";

export default function(pi: ExtensionAPI): void {
    const file = process.env["DOTS_TUI_MARKER"];
    if (!file) throw new Error("Missing terminal-test marker path");
    const mark = (text: string): void => { appendFileSync(file, text + "\n"); };
    pi.on("session_start", () => mark("start"));
    pi.on("session_shutdown", () => mark("shutdown"));
    pi.on("agent_start", () => mark("UNEXPECTED_PROVIDER_TURN"));
    pi.registerCommand("accept-theme", {
        async handler(_args, ctx) { mark("theme " + ctx.ui.setTheme("light").success); },
    });
    pi.registerCommand("accept-exit", {
        async handler(_args, ctx) { ctx.shutdown(); },
    });
}
