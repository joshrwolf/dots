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
- **↑/↓**, **PgUp/PgDn**, **Home/End** scroll; fullscreen also supports the wheel.
- **S** confirms a simulated job stop. **Esc** closes the workbench.

Letter shortcuts accept either case and supported terminal key protocols.
The initial palette follows the terminal’s light/dark appearance. **T** previews
the other palette on a matching panel surface; closing restores the initial palette.

Resize the real terminal to check layout. Reopen with `/ui-lab [scenario]`;
use `/ui-lab-exit` to exit. Source changes load with `/reload`; launcher changes
require relaunching the workbench. No installation is needed. For everyday Pi
sessions, use the [install and reload workflow](../../../docs/pi.md#updates).
Scenarios cover tools, nested codemode activity, job inspection, workers, thinking, and the
application frame (the default), including its native editor. The frame uses Pi’s
actual tool/message components through the shipping renderer resolver: calls,
job completions, worker replies, wakes, and an otherwise unstyled tool share one
transcript. `/ui-lab thinking` previews summaries, structured detail, and unfinished
streaming Markdown through Pi’s real assistant component; **E** toggles its native
thinking disclosure. Expand and scroll to compare summaries with recorded evidence. Wake scenarios show
scheduled follow-ups and failed checks. Input outside commands is ignored, not sent to a model.

`mise run //src/pi:verify-terminal` exercises real-host interaction in regular
and fullscreen modes. This directory is excluded from the published package.
