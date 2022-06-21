local M = {}

vim.o.completeopt = "menu,menuone,noinsert"

local types = require "cmp.types"

local kind_icons = {
  Text = "",
  Method = "",
  Function = "",
  Constructor = "",
  Field = "",
  Variable = "",
  Class = "ﴯ",
  Interface = "",
  Module = "",
  Property = "ﰠ",
  Unit = "",
  Value = "",
  Enum = "",
  Keyword = "",
  Snippet = "",
  Color = "",
  File = "",
  Reference = "",
  Folder = "",
  EnumMember = "",
  Constant = "",
  Struct = "",
  Event = "",
  Operator = "",
  TypeParameter = "",
}

function M.setup()
  local luasnip = require "luasnip"
  -- local neogen = require "neogen"
  local cmp = require "cmp"

  cmp.setup {
    completion = { completeopt = "menu,menuone,noinsert", keyword_length = 1 },
    snippet = {
      expand = function(args)
        require("luasnip").lsp_expand(args.body)
      end,
    },
    formatting = {
      format = function(entry, vim_item)
        -- Kind icons
        vim_item.kind = string.format("%s %s", kind_icons[vim_item.kind], vim_item.kind) -- This concatenates the icons with the name of the item kind
        --Source
        vim_item.menu = ({
          nvim_lsp = "[LSP]",
          luasnip = "[Snip]",
          buffer = "[Buf]",
          nvim_lua = "[nLua]",
          treesitter = "[TS]",
          path = "[Path]",
          nvim_lsp_signature_help = "[Sig]",
        })[entry.source.name]
        return vim_item
      end,
    },
    mapping = {
      ["<C-p>"] = {
        i = cmp.mapping.select_prev_item { behavior = types.cmp.SelectBehavior.NoInsert },
      },
      ["<C-n>"] = {
        i = cmp.mapping.select_next_item { behavior = types.cmp.SelectBehavior.NoInsert },
      },
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
      { name = "nvim_lsp" },
      { name = "nvim_lsp_signature_help" },
      { name = "luasnip" },
      { name = "treesitter" },
      { name = "buffer" },
      { name = "nvim_lua" },
    },
    window = {
      documentation = {
        border = { "╭", "─", "╮", "│", "╯", "─", "╰", "│" },
        winhighlight = "NormalFloat:NormalFloat,FloatBorder:TelescopeBorder",
      },
    },
  }

  -- Use buffer source for `/`
  cmp.setup.cmdline("/", {
    sources = {
      { name = "buffer" },
    },
  })

  -- Auto pairs
  local cmp_autopairs = require "nvim-autopairs.completion.cmp"
  cmp.event:on("confirm_done", cmp_autopairs.on_confirm_done { map_char = { tex = "" } })
end

return M
