---@meta
-- Editor and checker types for the `rex` global. Rex loads none of this.
-- Shapes follow `rex api describe <method>`; fields that code here does not
-- read stay `table`. Extend a class when a module starts depending on a field.

---@alias rex.Err string? # Set when a call fails; the result is then nil.

---@class rex.Context
---@field session_id? string
---@field block_id? string
---@field client_id? string
---@field origin? "key"|"palette"|"cli"|"api"|"script"|"timer"
---@field server? string

---@alias rex.KeyFn fun(ctx: rex.Context, ev: table): boolean?

---@alias rex.ArgType "string"|"number"|"integer"|"boolean"|"object"|"array"|"any"|"string?"|"number?"|"integer?"|"boolean?"|"object?"|"array?"|"any?"

---@class rex.ActionSpec
---@field name string Lowercase letters, digits, and underscores.
---@field title string
---@field run fun(ctx: rex.Context, args: table): any
---@field description? string
---@field category? string
---@field keywords? string
---@field args? table<string, rex.ArgType>|table A type table or a JSON Schema.

---@class rex.LayoutNode

---@class rex.TerminalOptions
---@field command? string[]
---@field shell? string
---@field cwd? string
---@field exit? { on_completion: boolean }

---@class rex.BlockSpec
---@field flavor string
---@field options? rex.TerminalOptions
---@field label? string

---@class rex.Layout
---@field block fun(spec: rex.BlockSpec): rex.LayoutNode

---@class rex.PlacedBlock
---@field block_id string
---@field label? string
---@field flavor? string
---@field creator_name? string

---@class rex.Layer
---@field kind "tiled"|"floating"
---@field layer_id? string
---@field blocks? rex.PlacedBlock[]

---@class rex.Window
---@field window_id string
---@field label? string
---@field focused_block_id? string
---@field zoomed_block_id? string
---@field layers rex.Layer[]

---@class rex.View
---@field session_id string
---@field label? string
---@field active_window_id? string
---@field windows rex.Window[]
---@field revision integer

---@class rex.NewWindowArgs
---@field layout rex.LayoutNode
---@field window_label? string
---@field focus? boolean
---@field session_id? string

---@class rex.FocusDirectionArgs
---@field direction "left"|"right"|"up"|"down"
---@field wrap? boolean
---@field session_id? string

-- Session helpers. They fill in `session_id`; other IDs are explicit. Methods
-- without a helper here, such as `session.attach`, go through `rex.call`.
---@class rex.SessionApi
---@field view fun(args?: { session_id?: string }): (rex.View?, rex.Err)
---@field new_window fun(args: rex.NewWindowArgs): (table?, rex.Err)
---@field new_layer fun(args: table): (table?, rex.Err)
---@field new_split fun(args: table): (table?, rex.Err)
---@field close_layer fun(args: table): (table?, rex.Err)
---@field focus_direction fun(args: rex.FocusDirectionArgs): (table?, rex.Err)
---@field focus_block fun(args: { block_id: string, session_id?: string }): (table?, rex.Err)
---@field zoom_block fun(args: table): (table?, rex.Err)

---@class rex.ExecOpts
---@field cwd? string
---@field env? table<string, string|false>
---@field timeout? number Seconds until SIGTERM, then SIGKILL two seconds later.

---@class rex.ExecResult
---@field code integer -1 when a signal ended the command.
---@field signal? string
---@field stdout string
---@field stderr string
---@field timed_out boolean
---@field ok boolean

---@class rex.Exec
---@overload fun(argv: string[], opts?: rex.ExecOpts): (rex.ExecResult?, string?)

---@class rex.BlockApi
---@field call fun(creator: string, method: string, args?: table): (table?, rex.Err)

---@class rex.Json
---@field encode fun(value: any, opts?: { indent?: boolean }): (string?, string?)
---@field decode fun(text: string): (any, string?)

---@class rex.Remote
---@field call fun(method: string, args?: table): (table?, rex.Err)

---@class rex.Api
---@field session rex.SessionApi
---@field block rex.BlockApi
---@field client { queue: fun(name: string, args?: table) }
---@field layout rex.Layout
---@field exec rex.Exec
---@field json rex.Json
---@field args table Script arguments; scripts only.
---@field stop? fun(code?: integer) Scripts only.
---@field after? fun(seconds: number, fn: fun(ctx: rex.Context)): table Scripts only.
rex = {}

---@param key string
---@param target string|rex.KeyFn An action name or a key function.
---@param args? table
function rex.bind(key, target, args) end

---@param spec rex.ActionSpec
function rex.action(spec) end

---@param name string
---@param args? table
---@return any result
---@return rex.Err
function rex.invoke(name, args) end

---@param method string
---@param args? table
---@return table?
---@return rex.Err
function rex.call(method, args) end

---@param event string
---@param fn fun(ctx: rex.Context, ev: table)
function rex.on(event, fn) end

---@return string[]
function rex.servers() end

---@param label string
---@return rex.Remote
function rex.server(label) end

---@param level_or_message string
---@param message? string
function rex.log(level_or_message, message) end
