local lualine = require('lualine')

local gps = require('nvim-gps')

lualine.setup({
    options = {
        icons_enabled = true,
        theme = 'nord',
        component_separators = { left = '', right = ''},
        section_separators = { left = '', right = ''},
        disabled_filetypes = {},
        always_divide_middle = true,
        globalstatus = false,
    },
    sections = {
        lualine_a = {'mode'},
        lualine_b = {'branch', 'diff', 'diagnostics'},
        lualine_c = {
           {
               'filename',
               path = 1,
               symbols = {
                   modified = '[+]',
                   readonly = '[-]',
                   unnamed = '[No Name]',
               },
           },
       },
       lualine_d = {
         {
           gps.get_location,
           cond = gps.is_available,
         }
       },
        lualine_x = {
            'encoding',
            'fileformat',
            { 'filetype', icons_enbled = true },
        },
        lualine_y = {'progress'},
        lualine_z = {'location'}
    },
    inactive_sections = {
        lualine_a = {},
        lualine_b = {},
        lualine_c = {'filename'},
        lualine_d = {},
        lualine_x = {'location'},
        lualine_y = {},
        lualine_z = {}
    },
    tabline = {},
    extensions = {}
})

