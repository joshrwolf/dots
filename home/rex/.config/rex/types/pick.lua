---@meta
-- The `dots-pick` view format, mirroring src/pick/src/spec.rs. Rust rejects
-- anything else; these types let the editor and lua-language-server catch the
-- same mistakes in view builders before they run.

---@alias pick.Role "strong"|"muted"|"accent"|"blue"|"green"|"red"|"yellow"

---@alias pick.CellKind "text"|"tag"|"path"

---@class pick.Cell
---@field text string
---@field role? pick.Role Defaults to strong.
---@field kind? pick.CellKind Defaults to text: drawn and searched.
---@field prefix? string Muted, unsearchable text before the cell's own.

---@alias pick.Width { cells: integer }|{ content: integer }|"fill"

---A group's columns line up by position with every other group's: a fixed
---column gets the widest size any group's fixed column at its position asks
---for, and a fill takes what its row leaves.
---@class pick.Column
---@field width pick.Width `cells` is at least that much text, not counting the gap after it; `content` fits the widest text at its position, up to that many cells.
---@field heading? string

---@class pick.Group
---@field id string
---@field label string
---@field columns pick.Column[]
---@field scope? string One character, unique among the view's groups, that shows the group when typed first.

---@class pick.Row
---@field id string Handed to the accept or shortcut action; unique in the view.
---@field group string A declared group's id.
---@field cells pick.Cell[] One per column of the group.
---@field search? string Search terms that are never drawn.
---@field preview? string[] A command whose output previews the row.

---@class pick.Shortcut
---@field key string A lowercase letter, pressed with Ctrl.
---@field label string
---@field action string Receives `{ id = row.id }`.
---@field query? boolean The typed line is input: the action receives `{ id?, query }`, `query` being the whole line and `id` the selected row, if any. A blank line asks for input instead of running the action.
---@field hint? string What to type, such as `"URL"`, shown after the label; only with `query`.

---@class pick.View
---@field prompt? string
---@field query? string
---@field accept string The action that receives `{ id = row.id }` when Enter chooses a row. Actions run while the picker stays open: success closes it, and an error message shows in it.
---@field shortcuts? pick.Shortcut[]
---@field groups pick.Group[]
---@field rows pick.Row[]
---@field watch? string[] A command printing a line whenever rows may change.
---@field preview_every? number Seconds between preview re-runs, 0.1 to 3600.
---@field preview_anchor? "start"|"end" Which part of long preview output shows; defaults to the end, as for terminal screens.
---@field preview_position? "right"|"below" Where the preview sits: right of the list (default), shown only on terminals at least 80 cells wide, or below it, where the list keeps 40% of the height.
