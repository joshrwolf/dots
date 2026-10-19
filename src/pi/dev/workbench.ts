import type { Theme } from "@earendil-works/pi-coding-agent";
import { Text, VStack, matchesKey } from "@earendil-works/pi-tui";
import { PriorityRow, labelText } from "../src/index.ts";
import { paintPreviewLine } from "./surface.ts";
import { scenarios, type ScenarioContext, type ScenarioView } from "./scenarios.ts";

export interface WorkbenchState {
  readonly scenario: string;
  readonly phase: number;
  readonly expanded: boolean;
}
export interface WorkbenchHost extends ScenarioContext {
  readonly theme: () => Theme;
  readonly toggleTheme: () => void;
  readonly confirmStop: (signal: AbortSignal) => Promise<boolean>;
  readonly changed?: (state: WorkbenchState) => void;
}

/** Development-only controller. States advance explicitly, not on guessed delays.
 * The content is the actual production component tree; Pi still owns the terminal. */
export class Workbench extends VStack {
  private readonly host: WorkbenchHost;
  private readonly heading = new PriorityRow();
  private readonly content = new VStack();
  private readonly help = new Text("", 0, 0);
  private selection = 0;
  private phase = 0;
  private expanded = false;
  private view: ScenarioView | undefined;
  private alive = true;
  private confirmation: AbortController | undefined;
  private generation = 0;

  constructor(host: WorkbenchHost, initial = "frame") {
    super();
    this.host = host;
    const selection = scenarios.findIndex((scenario) => scenario.id === initial);
    if (selection < 0) throw new Error(`Unknown scenario ${initial}; choose ${scenarios.map((s) => s.id).join(", ")}`);
    this.selection = selection;
    this.addChild(this.heading);
    this.addChild(this.content, { grow: 1, minSize: 1, maxSize: 34 });
    this.addChild(this.help);
    this.refresh();
  }

  get state(): WorkbenchState { return { scenario: scenarios[this.selection]!.id, phase: this.phase, expanded: this.expanded }; }

  private refresh(): void {
    if (!this.alive) return;
    const scenario = scenarios[this.selection]!;
    const theme = this.host.theme();
    if (!this.view) {
      const generation = ++this.generation;
      this.view = scenario.create({ ...this.host, redraw: () => {
        if (this.alive && generation === this.generation) this.host.redraw();
      } }, theme);
      this.content.clear();
      this.content.addChild(this.view.component);
    }
    this.view.update(this.phase, this.expanded, theme);
    this.heading.update({ layout: "inline", groupSeparator: "  ", start: [
      { text: "UI workbench", tone: "accent", bold: true, priority: 100, shrink: "clip" },
    ], end: [{ text: `${this.selection + 1}/${scenarios.length} · ${scenario.id} · ${scenario.phases[this.phase]} · ${this.expanded ? "expanded" : "collapsed"}`, tone: "muted", priority: 80, shrink: "clip" }] }, theme);
    this.help.setText(theme.fg("dim", "N/P scenario · Space state · E expand · T theme · S simulated stop · Esc close"));
    this.invalidate();
    this.host.redraw();
    this.host.changed?.(this.state);
  }

  override render(width: number): string[] {
    if (!this.alive || width <= 0) return [];
    const theme = this.host.theme();
    // A preview owns its surface: switching palette must not put dark-theme
    // foregrounds directly on a light terminal (or vice versa).
    return super.render(width).map((line) => paintPreviewLine(line, width, theme));
  }

  handleInput(data: string): void {
    if (!this.alive) return;
    const pressed = (key: "n" | "p" | "e" | "t" | "s") => matchesKey(data, key) || matchesKey(data, `shift+${key}`) || data === key.toUpperCase();
    if (pressed("n") || pressed("p")) {
      this.selection = (this.selection + (pressed("n") ? 1 : scenarios.length - 1)) % scenarios.length;
      this.phase = 0; this.view = undefined;
    } else if (matchesKey(data, "space")) this.phase = (this.phase + 1) % scenarios[this.selection]!.phases.length;
    else if (pressed("e")) this.expanded = !this.expanded;
    else if (pressed("t")) this.host.toggleTheme();
    else if (pressed("s")) { void this.requestStop(); return; }
    else { this.view?.component.handleInput?.(data); return; }
    this.refresh();
  }

  async requestStop(): Promise<void> {
    const view = this.view;
    if (!this.alive || this.confirmation || !view?.canAct?.()) return;
    const phase = this.phase;
    const controller = new AbortController();
    this.confirmation = controller;
    try {
      const approved = await this.host.confirmStop(controller.signal);
      if (approved && this.alive && !controller.signal.aborted && this.view === view && this.phase === phase && view.canAct?.()) {
        const nextPhase = view.act?.();
        if (nextPhase !== undefined) this.phase = nextPhase;
        this.refresh();
      }
    } catch (error) {
      if (this.alive && !controller.signal.aborted) {
        this.help.setText(this.host.theme().fg("error", labelText(`Confirmation failed: ${String(error)}`)));
        this.host.redraw();
      }
    } finally {
      if (this.confirmation === controller) this.confirmation = undefined;
    }
  }

  dispose(): void {
    if (!this.alive) return;
    this.alive = false;
    this.generation++;
    this.confirmation?.abort();
    this.confirmation = undefined;
    this.clear();
    this.view = undefined;
  }
}
