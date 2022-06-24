local M = {}

vim.o.completeopt = "menu,menuone,noinsert"

local types = require "cmp.types"
local lspkind = require("lspkind")

local icons = {
	Text = "  ",
	Method = "  ",
	Function = "  ",
	Constructor = "⌘  ",
	Field = "ﰠ  ",
	Variable = "  ",
	Class = "ﴯ  ",
	Interface = "  ",
	Module = "  ",
	Property = "ﰠ  ",
	Unit = "塞 ",
	Value = "  ",
	Enum = "  ",
	Keyword = "廓 ",
	Snippet = "  ",
	Color = "  ",
	File = "  ",
	Reference = "  ",
	Folder = "  ",
	EnumMember = "  ",
	Constant = "  ",
	Struct = "פּ  ",
	Event = " ",
	Operator = "  ",
	TypeParameter = "t ",
}

-- vim.highlight.create("PmenuSel", { guifg = "NONE", guibg = "#282C34" }, false)
-- vim.highlight.create("Pmenu", { guifg = "#C5CDD9", guibg = "#22252A" }, false)
--
-- vim.highlight.create("CmpItemAbbrDeprecated", { guifg = "#7E8294", guibg = "NONE", gui = "strikethrough" }, false)
-- vim.highlight.create("CmpItemAbbrMatch", { guifg = "#82AAFF", guibg = "NONE", gui = "bold" }, false)
-- vim.highlight.create("CmpItemAbbrMatchFuzzy", { guifg = "#82AAFF", guibg = "NONE", gui = "bold" }, false)
-- vim.highlight.create("CmpItemMenu", { guifg = "#C792EA", guibg = "NONE", gui = "italic" }, false)
--
-- vim.highlight.create("CmpItemKindField", { guifg = "#EED8DA", guibg = "#B5585F" }, false)
-- vim.highlight.create("CmpItemKindProperty", { guifg = "#EED8DA", guibg = "#B5585F" }, false)
-- vim.highlight.create("CmpItemKindEvent", { guifg = "#EED8DA", guibg = "#B5585F" }, false)
--
-- vim.highlight.create("CmpItemKindText", { guifg = "#C3E88D", guibg = "#9FBD73" }, false)
-- vim.highlight.create("CmpItemKindEnum", { guifg = "#C3E88D", guibg = "#9FBD73" }, false)
-- vim.highlight.create("CmpItemKindKeyword", { guifg = "#C3E88D", guibg = "#9FBD73" }, false)
--
-- vim.highlight.create("CmpItemKindConstant", { guifg = "#FFE082", guibg = "#D4BB6C" }, false)
-- vim.highlight.create("CmpItemKindConstructor", { guifg = "#FFE082", guibg = "#D4BB6C" }, false)
-- vim.highlight.create("CmpItemKindReference", { guifg = "#FFE082", guibg = "#D4BB6C" }, false)
--
-- vim.highlight.create("CmpItemKindFunction", { guifg = "#EADFF0", guibg = "#A377BF" }, false)
-- vim.highlight.create("CmpItemKindStruct", { guifg = "#EADFF0", guibg = "#A377BF" }, false)
-- vim.highlight.create("CmpItemKindClass", { guifg = "#EADFF0", guibg = "#A377BF" }, false)
-- vim.highlight.create("CmpItemKindModule", { guifg = "#EADFF0", guibg = "#A377BF" }, false)
-- vim.highlight.create("CmpItemKindOperator", { guifg = "#EADFF0", guibg = "#A377BF" }, false)
--
-- vim.highlight.create("CmpItemKindVariable", { guifg = "#C5CDD9", guibg = "#7E8294" }, false)
-- vim.highlight.create("CmpItemKindFile", { guifg = "#C5CDD9", guibg = "#7E8294" }, false)
--
-- vim.highlight.create("CmpItemKindUnit", { guifg = "#F5EBD9", guibg = "#D4A959" }, false)
-- vim.highlight.create("CmpItemKindSnippet", { guifg = "#F5EBD9", guibg = "#D4A959" }, false)
-- vim.highlight.create("CmpItemKindFolder", { guifg = "#F5EBD9", guibg = "#D4A959" }, false)
--
-- vim.highlight.create("CmpItemKindMethod", { guifg = "#DDE5F5", guibg = "#6C8ED4" }, false)
-- vim.highlight.create("CmpItemKindValue", { guifg = "#DDE5F5", guibg = "#6C8ED4" }, false)
-- vim.highlight.create("CmpItemKindEnumMember", { guifg = "#DDE5F5", guibg = "#6C8ED4" }, false)
--
-- vim.highlight.create("CmpItemKindInterface", { guifg = "#D8EEEB", guibg = "#58B5A8" }, false)
-- vim.highlight.create("CmpItemKindColor", { guifg = "#D8EEEB", guibg = "#58B5A8" }, false)
-- vim.highlight.create("CmpItemKindTypeParameter", { guifg = "#D8EEEB", guibg = "#58B5A8" }, false)

function M.setup()
  local luasnip = require "luasnip"
  -- local neogen = require "neogen"
  local cmp = require "cmp"

  cmp.setup {
    completion = {
      completeopt = "menu,menuone,noinsert",
      autocomplete = { types.cmp.TriggerEvent.TextChanged },
      keyword_length = 1,
    },
    snippet = {
      expand = function(args)
        require("luasnip").lsp_expand(args.body)
      end,
    },
    formatting = {
      fields = { "kind", "abbr", "menu" },
      -- format = lspkind.cmp_format({
      --   icon = true,
      --   with_text = true,
      --   before = function(entry, vim_item)
      --     local duplicates = {
      --       buffers = 1,
      --       luasnip = 1,
      --       nvim_lsp = 0,
      --       nvim_lua = 1,
      --       nvim_lsp_document_symbol = 1,
      --       nvim_lsp_signature_help = 1,
      --       path = 1,
      --     }
      --
      --     vim_item.dup = duplicates[entry.source_name] or 0
      --     vim_item.kind = (icons[vim_item.kind] or '') .. vim_item.kind
      --     return vim_item
      --   end
      -- }),
      format = function (entry, vim_item)
        local kind = require("lspkind").cmp_format({ mode = "symbol_text", maxwidth = 50 })(entry, vim_item) 
        local strings = vim.split(kind.kind, "%s", { trimempty = true })
        kind.kind = " " .. strings[1] .. " "
        kind.menu = "    (" .. strings[2] .. ")"
        return kind
      end
    },
    mapping = {
      ["<C-p>"] = {
        i = cmp.mapping.select_prev_item { behavior = types.cmp.SelectBehavior.NoInsert },
      },
      ["<C-n>"] = {
        i = cmp.mapping.select_next_item { behavior = types.cmp.SelectBehavior.NoInsert },
      },
      ["<C-u>"] = cmp.mapping.scroll_docs(-4),
      ["<C-d>"] = cmp.mapping.scroll_docs(4),
      ["<C-Space>"] = cmp.mapping(cmp.mapping.complete(), { "i", "c" }),
      ["<CR>"] = cmp.mapping.confirm { select = false },
      ["<Tab>"] = cmp.mapping(function(fallback)
        if cmp.visible() then
          local entry = cmp.get_selected_entry()
          if not entry then
            cmp.select_next_item({ behavior = cmp.SelectBehavior.Select })
          else
            cmp.confirm()
          end
        else
          fallback()
        end
      end),
    },
    sources = {
      { name = "nvim_lsp", priority_weight = 110, group_index = 1, max_item_count = 25 },
      { name = "luasnip", priority_weight = 105, group_index = 1 },
      { name = "treesitter", priority_weight = 102, group_index = 1, max_item_count = 10, keyword_length = 2 },
      { name = "nvim_lsp_document_symbol", priority_weight = 98, group_index = 1 },
      { name = "nvim_lsp_signature_help", priority_weight = 97, group_index = 2 },
      { name = "nvim_lua", priority_weight = 96, grou_index = 2 },
      { name = "buffer", priority_weight = 70, group_index = 2 },
    },
    window = {
      completion = {
        winhighlight = "Normal:Pmenu,FloatBorder:Pmenu,Search:None",
        col_offset = -3,
        side_padding = 0,
      },
      -- completion = vim.tbl_deep_extend("force", cmp.config.window.bordered(), { col_offset = -1 }),
      documentation = cmp.config.window.bordered(),
      -- documentation = {
      --   -- border = { "╭", "─", "╮", "│", "╯", "─", "╰", "│" },
      --   -- winhighlight = "NormalFloat:NormalFloat,FloatBorder:TelescopeBorder",
      -- },
    },
    confirm_opts = {
      behavior = cmp.ConfirmBehavior.Replace,
      select = false,
    },
    experimental = { ghost_text = true },

  }

  -- Use buffer source for `/`
  cmp.setup.cmdline("/", {
    mapping = cmp.mapping.preset.cmdline(),
    sources = cmp.config.sources(
      {
        { name = "buffer" },
      },
      {
        { name = "nvim_lsp_document_symbol" },
      }
    )
  })

  -- Auto pairs
  local cmp_autopairs = require "nvim-autopairs.completion.cmp"
  cmp.event:on("confirm_done", cmp_autopairs.on_confirm_done { map_char = { tex = "" } })
end

return M
