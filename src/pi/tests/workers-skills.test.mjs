import assert from 'node:assert/strict';
import {mkdirSync, mkdtempSync, rmSync, writeFileSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import test from 'node:test';
import {
  selectableSkills,
  selectSkills,
  skillContext,
} from '../extensions/workers/skills.ts';
import {WorkerRecordSchema, decode} from '../extensions/workers/record.ts';

/** Prompt-option skills as Pi's before_agent_start reports them. */
const skill = (name, filePath, disableModelInvocation = false) => ({
  name,
  description: `${name} workflow`,
  filePath,
  baseDir: filePath,
  sourceInfo: {
    path: filePath,
    source: 'local',
    scope: 'user',
    origin: 'top-level',
  },
  disableModelInvocation,
});
const skills = [
  skill('alpha', '/skills/alpha/SKILL.md'),
  skill('hidden', '/skills/hidden/SKILL.md', true),
  skill('beta', '/project/.agents/skills/beta/SKILL.md'),
  skill('virtual', '<inline:virtual>'),
];

test('the selectable skills are the model-invocable prompt-option skills', () => {
  assert.deepEqual(selectableSkills(skills), [
    {name: 'alpha', path: '/skills/alpha/SKILL.md'},
    {name: 'beta', path: '/project/.agents/skills/beta/SKILL.md'},
    {name: 'virtual', path: '<inline:virtual>'},
  ]);
  assert.deepEqual(selectableSkills([]), []);
});

test('selection resolves names in order, once, and lists only selectable files on a miss', () => {
  const selectable = selectableSkills(skills);
  assert.deepEqual(selectSkills(['beta', 'alpha', 'beta'], selectable), [
    {name: 'beta', path: '/project/.agents/skills/beta/SKILL.md'},
    {name: 'alpha', path: '/skills/alpha/SKILL.md'},
  ]);
  assert.throws(
    () => selectSkills(['alpha', 'alhpa'], selectable),
    /^Error: Unknown skill: alhpa\. Available skills: alpha, beta\.$/,
  );
  assert.throws(
    () => selectSkills(['hidden'], selectable),
    error =>
      /^Unknown skill: hidden\. Available skills: alpha, beta\.$/.test(
        error.message,
      ) && !error.message.includes('/skills/hidden'),
    'a command-only skill is unknown to selection',
  );
  assert.throws(
    () => selectSkills(['x', 'y'], selectable),
    /Unknown skills: x, y\./,
  );
  assert.throws(
    () => selectSkills(['alpha'], []),
    /This session has no available skills/,
  );
  assert.throws(
    () => selectSkills(['x'], selectableSkills([skill('virtual', '<v>')])),
    /This session has no available skills/,
  );
});

test('only a selected skill without an absolute file blocks selection', () => {
  const selectable = selectableSkills(skills);
  assert.deepEqual(selectSkills(['alpha'], selectable), [
    {name: 'alpha', path: '/skills/alpha/SKILL.md'},
  ]);
  assert.throws(
    () => selectSkills(['alpha', 'virtual'], selectable),
    /^Error: Pi reports no skill file to hand off for virtual \("<inline:virtual>"\)\.$/,
  );
  assert.throws(
    () =>
      selectSkills(
        ['relative', 'nope'],
        [
          {name: 'relative', path: 'skills/relative/SKILL.md'},
          {name: 'alpha', path: '/skills/alpha/SKILL.md'},
        ],
      ),
    /^Error: Unknown skill: nope\. Available skills: alpha\. Pi reports no skill file to hand off for relative \("skills\/relative\/SKILL\.md"\)\.$/,
  );
});

/** Real skill directories with a referenced file the handoff must not load. */
function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), 'dots-skills-unit-'));
  t.after(() => rmSync(root, {recursive: true, force: true}));
  const write = (name, body) => {
    const dir = join(root, name);
    mkdirSync(join(dir, 'references'), {recursive: true});
    writeFileSync(join(dir, 'references/detail.md'), `DETAIL-${name}\n`);
    const file = join(dir, 'SKILL.md');
    writeFileSync(file, body);
    return {dir, file};
  };
  return {root, write};
}

test('the handoff loads each selected file in full, in selection order', t => {
  assert.equal(skillContext(undefined), '');
  assert.equal(skillContext([]), '');
  const {write} = fixture(t);
  const alpha = write(
    'alpha',
    '---\nname: alpha\ndescription: Alpha workflow.\n---\n\n# Alpha\n\nRead `references/detail.md` when needed.\n\n',
  );
  const beta = write('beta', 'BODY-beta with no frontmatter');
  write('gamma', 'BODY-gamma unselected');
  const selectable = [
    {name: 'alpha', path: alpha.file},
    {name: 'beta', path: beta.file},
  ];
  const text = skillContext(
    selectSkills(['beta', 'alpha', 'beta'], selectable),
  );
  const [lead, ...blocks] = text.split(/\n\n(?=<skill )/);
  assert.match(lead, /^Skills selected by the parent for this assignment/);
  assert.doesNotMatch(lead, /\bread\b/i, 'no instruction to read the skills');
  assert.deepEqual(blocks, [
    `<skill name="beta" location="${beta.file}">\nReferences are relative to ${beta.dir}.\n\nBODY-beta with no frontmatter\n</skill>`,
    `<skill name="alpha" location="${alpha.file}">\nReferences are relative to ${alpha.dir}.\n\n---\nname: alpha\ndescription: Alpha workflow.\n---\n\n# Alpha\n\nRead \`references/detail.md\` when needed.\n</skill>`,
  ]);
  assert.doesNotMatch(text, /DETAIL-|BODY-gamma/, 'references stay lazy');
});

test('the handoff reads current files and names every unreadable one', t => {
  const {root, write} = fixture(t);
  const alpha = write('alpha', 'first');
  const refs = [{name: 'alpha', path: alpha.file}];
  assert.match(skillContext(refs), /\n\nfirst\n<\/skill>$/);
  writeFileSync(alpha.file, 'second');
  assert.match(skillContext(refs), /\n\nsecond\n<\/skill>$/, 'no snapshot');
  const gone = join(root, 'gone/SKILL.md');
  assert.throws(
    () =>
      skillContext([
        {name: 'gone', path: gone},
        ...refs,
        {name: 'dir', path: root},
      ]),
    error =>
      error.message.startsWith(
        `Cannot read selected skill files: gone (${gone}): ENOENT`,
      ) &&
      error.message.includes(`; dir (${root}): EISDIR`) &&
      !error.message.includes('alpha'),
  );
  assert.throws(
    () => skillContext([{name: 'gone', path: gone}]),
    /^Error: Cannot read selected skill file: gone \(/,
  );
});

test('worker records retain selected skill references and reject malformed ones', () => {
  const record = {
    version: 1,
    owner: 'session',
    id: 'w-0123456789abcdef',
    label: 'skilled',
    cwd: '/work',
    sessionDir: '/sessions/w-0123456789abcdef',
    state: 'completed',
    startedAt: 1,
    updatedAt: 2,
    calls: 0,
    tokens: 0,
    settlements: 1,
  };
  assert.equal(decode(WorkerRecordSchema, record).ok, true);
  const skills = [{name: 'alpha', path: '/skills/alpha/SKILL.md'}];
  assert.deepEqual(decode(WorkerRecordSchema, {...record, skills}), {
    ok: true,
    value: {...record, skills},
  });
  for (const broken of [['alpha'], [{name: 'alpha'}], [{...skills[0], x: 1}]])
    assert.equal(
      decode(WorkerRecordSchema, {...record, skills: broken}).ok,
      false,
    );
});
