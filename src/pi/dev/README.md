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
Scenarios cover tools, nested codemode activity, job inspection, workers, thinking, and the
application frame (the default), including its native editor. The frame uses Pi’s
actual tool/message components through the shipping renderer resolver: calls,
job completions, worker replies, wakes, and an otherwise unstyled tool share one
transcript. `/ui-lab thinking` previews summaries, structured detail, and unfinished
streaming Markdown through Pi’s real assistant component; **E** toggles its native
thinking disclosure. `/ui-lab goal` starts with nested groups in the framed dashboard.
Space cycles through closed history, partial trees, multiple follows, held work,
unavailable data, and brief warnings. In its
inspector, **[**/**]** select groups and **←**/**→** fold/unfold them; Space remains
the workbench state switch.
**E** switches between the persistent panel and slice inspection; the frame
scenario also shows the goal panel below the native editor. Its footer uses
foreground colors and Unicode icons, never Powerline bars. Space cycles
through ordinary, warning, critical, unknown, long-label, publisher-overflow, and
no-model states. The preview uses the same footer component as the dots UI;
fixture state never changes your session or saved preferences. Expand and scroll to compare summaries with recorded evidence. Wake scenarios show
scheduled follow-ups and failed checks. Input outside commands is ignored, not sent to a model.

`mise run //src/pi:verify-terminal` exercises the real interactive host, through
Pi's virtual-terminal option, in regular and fullscreen modes. The companion
manifest does not load this directory.
