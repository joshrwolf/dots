import { displayLimits } from "../policy.ts";
import { getLanguageFromPath } from "@earendil-works/pi-coding-agent";
import { bundledLanguages, createHighlighter, type BundledLanguage } from "shiki";

export interface SyntaxToken { readonly content: string; readonly offset: number; readonly color?: string; readonly fontStyle?: number }
export type Tokens = readonly (readonly SyntaxToken[])[];
export interface SyntaxService {
  readonly active: boolean;
  readonly failure: unknown;
  highlight(source: string, path: string, appearance: "light" | "dark"): Promise<Tokens>;
  dispose(): void;
}
export function plainTokens(source: string): Tokens { return source.split("\n").map((content) => [{ content, offset: 0 }]); }

export interface SyntaxEngine {
  load(language: string): Promise<void>;
  tokenize(source: string, language: string, appearance: "light" | "dark"): Tokens;
  dispose(): void;
}
async function createShikiEngine(): Promise<SyntaxEngine> {
  const engine = await createHighlighter({ langs: [], themes: ["github-light", "github-dark"] });
  return {
    load: (language) => engine.loadLanguage(language as BundledLanguage),
    tokenize: (source, language, appearance) => engine.codeToTokensBase(source, { lang: language as BundledLanguage, theme: appearance === "light" ? "github-light" : "github-dark" }),
    dispose: () => engine.dispose(),
  };
}

/** Available before session_start; construction itself starts no work. */
export function createSyntaxService(create: () => Promise<SyntaxEngine> = createShikiEngine): SyntaxService {
  let active = true;
  let failure: unknown;
  let engine: Promise<SyntaxEngine> | undefined;
  const languages = new Map<string, Promise<void>>();
  return {
    get active() { return active; },
    get failure() { return failure; },
    async highlight(source, path, appearance) {
      const language = getLanguageFromPath(path);
      if (!active || !language || !Object.hasOwn(bundledLanguages, language) || source.length > displayLimits.syntaxCodeUnits || source.split("\n").some((line) => line.length > displayLimits.syntaxLineCodeUnits)) return plainTokens(source);
      try {
        if (!engine) {
          engine = create();
        }
        const value = await engine;
        if (!active) return plainTokens(source);
        let loading = languages.get(language);
        if (!loading) {
          loading = value.load(language);
          languages.set(language, loading);
        }
        await loading;
        if (!active) return plainTokens(source);
        return value.tokenize(source, language, appearance);
      } catch (error) {
        failure = error;
        return plainTokens(source);
      }
    },
    dispose() {
      if (!active) return;
      active = false;
      // Release exactly once, including initialization that finishes after shutdown.
      const pending = [...languages.values()];
      if (engine) void engine.then(async (value) => {
        await Promise.allSettled(pending);
        value.dispose();
      }, () => {});
      languages.clear();
    },
  };
}
