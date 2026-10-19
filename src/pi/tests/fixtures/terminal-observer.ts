import {appendFileSync} from "node:fs";
import type {ExtensionAPI} from "@earendil-works/pi-coding-agent";

export default function(pi: ExtensionAPI): void {
    const file = process.env["DOTS_TUI_MARKER"];
    if (!file) throw new Error("Missing terminal-test marker path");
    const mark = (text: string): void => { appendFileSync(file, text + "\n"); };
    const subscriptions: (() => void)[] = [];
    pi.on("session_start", (_event, ctx) => {
        mark("start");
        if (process.env["DOTS_UI_WORKBENCH"] !== "1") return;
        subscriptions.push(
            pi.events.on("dots:workbench", (state) => mark("workbench " + JSON.stringify(state))),
            pi.events.on("dots:workbench-closed", () => mark("workbench closed")),
            pi.events.on("dots:workbench-input", () => mark("workbench input handled")),
            pi.events.on("dots:workbench-confirmed", (approved) => mark("confirmation " + String(approved))),
        );
        let previous = "";
        ctx.ui.setWidget("accept-render-probe", () => ({
            invalidate() {},
            render(width) {
                const next = `frame ${width} ${ctx.ui.theme.appearance}`;
                if (previous !== next) { previous = next; mark(next); }
                return [];
            },
        }));
    });
    pi.on("session_shutdown", () => { for (const unsubscribe of subscriptions.splice(0)) unsubscribe(); mark("shutdown"); });
    pi.on("agent_start", () => mark("UNEXPECTED_PROVIDER_TURN"));
    pi.registerCommand("accept-theme", {
        async handler(_args, ctx) { mark("theme " + ctx.ui.setTheme("light").success); },
    });
    pi.registerCommand("accept-exit", {
        async handler(_args, ctx) { ctx.shutdown(); },
    });
}
