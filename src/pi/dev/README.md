# UI workbench

Preview the shipping presentation components inside stock Pi, with a disposable
home, neutral fixtures, no provider turns, and no jobs or workers launched.
Nothing is installed.

```sh
mise run //src/pi:workbench
mise run //src/pi:workbench -- regular job
```

Press Enter to open the prefilled `/ui-lab` command. Inside the workbench:

- **N/P** selects a scenario; **Space** advances its recorded state.
- **E** expands details; **T** switches light/dark themes.
- **B** scrolls to the bottom. In **frame**, this reveals the editor, goals, and
  icon footer; use `/ui-lab frame` to open that preview directly.
- **↑/↓**, **PgUp/PgDn**, **Home/End** scroll; fullscreen also supports the wheel.
- **S** confirms a simulated job stop. **Esc** closes the workbench.

Letter shortcuts accept either case and supported terminal key protocols.
The initial palette follows the terminal’s light/dark appearance. **T** previews
the other palette on a matching panel surface; closing restores the initial palette.

Resize the real terminal to check layout. Reopen with `/ui-lab [scenario]`;
use `/ui-lab-exit` to exit. Source changes load with `/reload`; launcher changes
require relaunching the workbench. For everyday Pi
sessions, use the [install and reload workflow](../../../docs/pi.md#updates).
The **workers** scenario previews spinning-dot activity, questions, quiet workers,
one/three/eight-worker fleets, compact outcomes, and long questions. Active workers
use two lines; finished workers use one. Worker model, effort, and horizontal
context meters share the primary session’s right-aligned metadata columns.
Space advances these cases.
`/ui-lab attention` previews the goal popup's Attention view over in-memory
requests; Space steps through its states and E passes keys to the popup.
`/ui-lab inspection-shell` previews the shipping framed Panel and native picker
at wide and narrow widths in light/dark themes. Ctrl+Space cycles its fixture
states without taking picker keys. Esc closes; Ctrl+N/P switches scenarios.
`/ui-lab worker-inspector` opens the interactive picker: Enter inspects, L returns
to the list, M composes a fixture message, and X confirms a fixture stop.
Descendants are read-only. Scroll to pause follow; **F** resumes. All actions stay
in memory. Ctrl+N/P switches scenarios without consuming message text.
Scenarios cover tools, nested codemode activity, job inspection, workers, thinking, and the
application frame (the default), including its native editor. For responsive tool layout,
open `/ui-lab bash`, `/ui-lab codemode`, and `/ui-lab edit`; advance to their long
command, nested preview, and long-line diff states with Space. Resize from narrow to
wide and back, toggle E for wrapped details, and T for the other theme. The frame uses Pi’s
actual tool/message components through the shipping renderer resolver: calls,
job completions, worker replies, wakes, and an otherwise unstyled tool share one
transcript. `/ui-lab thinking` previews summaries, structured detail, and unfinished
streaming Markdown through Pi’s real assistant component; **E** toggles its native
thinking disclosure. `/ui-lab goal` shows the goal summary line below the native editor.
**E** opens the goal popup centered over the transcript and editor, at the size
Alt+G opens it in a session. It starts with a 12-task DAG: shared prerequisites,
parallel CLI and SDK branches, and joins at migration, packaging, and publication.
All tasks are visible by default; **h** hides or shows closed tasks.
In the popup, **1/2/3**, **h**, **o**, **↑**/**↓** and
**←**/**→** work as in a session; Space remains the workbench state switch and
cycles running, asking and quiet workers, complete and partial trees, multiple
follows, held work, unavailable data, and brief warnings. The **frame**
scenario starts with a live assigned goal, asking/running workers, a job, and an update notice
together below the native editor. Press **B** to see them. **Space** also reaches
crowded and settled activity states, plus context warnings, unknown usage, long
labels, and missing models. The footer uses foreground colors and Unicode icons,
never Powerline bars. The preview uses the same footer component as the dots UI;
fixture state never changes your session or saved preferences. Expand and scroll to compare summaries with recorded evidence. Wake scenarios show
scheduled follow-ups, failed checks, the persistent Wakes section, checking and
queued states, overflow, and full inspection. Input outside commands is ignored, not sent to a model.

`mise run //src/pi:verify-terminal` exercises the real interactive host, through
Pi's virtual-terminal option, in regular and fullscreen modes. The companion
manifest does not load this directory.
