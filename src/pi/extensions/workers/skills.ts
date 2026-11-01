import {readFileSync} from 'node:fs';
import path from 'node:path';
import type {Skill} from '@earendil-works/pi-coding-agent';
import type {Static} from 'typebox';
import type {SkillRefSchema} from './record.ts';

/** A skill as the host named and located it; a selected skill's path is an absolute file. */
export type SkillRef = Static<typeof SkillRefSchema>;

/**
 * The skills the model may select: a run's prompt-option skills, which Pi
 * loads independent of `enableSkillCommands`, without those it hides from model
 * invocation (`disable-model-invocation`).
 */
export function selectableSkills(
  skills: readonly Pick<
    Skill,
    'name' | 'filePath' | 'disableModelInvocation'
  >[],
): SkillRef[] {
  return skills.flatMap(skill =>
    skill.disableModelInvocation
      ? []
      : [{name: skill.name, path: skill.filePath}],
  );
}

/**
 * Resolves selected names against the selectable skills, in order and once
 * each. A miss names the valid choices: skills with an absolute file. Selecting
 * one without a file (an SDK host can inject virtual skills) is reported.
 */
export function selectSkills(
  names: readonly string[],
  selectable: readonly SkillRef[],
): SkillRef[] {
  const byName = new Map(selectable.map(skill => [skill.name, skill]));
  const selected = new Map<string, SkillRef>();
  const unknown: string[] = [];
  const fileless: string[] = [];
  for (const name of names) {
    const skill = byName.get(name);
    if (!skill) unknown.push(name);
    else if (!path.isAbsolute(skill.path))
      fileless.push(`${name} (${JSON.stringify(skill.path)})`);
    else selected.set(name, skill);
  }
  const problems: string[] = [];
  if (unknown.length) {
    const choices = selectable
      .filter(skill => path.isAbsolute(skill.path))
      .map(skill => skill.name)
      .sort();
    problems.push(
      `Unknown skill${unknown.length === 1 ? '' : 's'}: ${unknown.join(', ')}. ${choices.length ? `Available skills: ${choices.join(', ')}` : 'This session has no available skills'}.`,
    );
  }
  if (fileless.length)
    problems.push(
      `Pi reports no skill file to hand off for ${fileless.join(', ')}.`,
    );
  if (problems.length) throw new Error(problems.join(' '));
  return [...selected.values()];
}

/**
 * The child's handoff: each selected skill file in full, read now, in skill
 * blocks with their source locations. Files a skill references stay unread. Every launch
 * reads the current files, so a restart picks up edits; an unreadable file
 * fails the launch, naming each skill and path, rather than handing off less.
 */
export function skillContext(skills: readonly SkillRef[] | undefined): string {
  if (!skills?.length) return '';
  const blocks: string[] = [];
  const unreadable: string[] = [];
  for (const skill of skills) {
    try {
      const body = readFileSync(skill.path, 'utf8').trimEnd();
      blocks.push(
        `<skill name="${skill.name}" location="${skill.path}">\nReferences are relative to ${path.dirname(skill.path)}.\n\n${body}\n</skill>`,
      );
    } catch (error) {
      unreadable.push(
        `${skill.name} (${skill.path}): ${error instanceof Error ? error.message : String(error)}`,
      );
    }
  }
  if (unreadable.length)
    throw new Error(
      `Cannot read selected skill file${unreadable.length === 1 ? '' : 's'}: ${unreadable.join('; ')}`,
    );
  return [
    'Skills selected by the parent for this assignment, loaded in full below. Apply each where it bears on the work; they inform how you work, not what the brief covers. Files a skill references are not loaded; resolve their paths against its directory.',
    ...blocks,
  ].join('\n\n');
}
