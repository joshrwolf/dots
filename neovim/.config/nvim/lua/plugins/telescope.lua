local status_ok, telescope = pcall(require, 'telescope')
if not status_ok then
    return
end

telescope.setup({
    pickers = {
        find_files = {
          hidden = true,
        },
    },
    defaults = {
        prompt_prefix = '🔍 ',
        file_ignore_patterns = {
            '.git/',
        },
        vimgrep_arguments = {
            "rg",
            "--follow",
            "--color=never",
            "--no-heading",
            "--with-filename",
            "--line-number",
            "--column",
            "--smart-case",
            "--no-ignore",
            "--hidden",
            "--trim",
        },
    },
    extensions = {
        fzf = {
            fuzzy = true,
            override_generic_sorter = true,
            override_file_sorter = true,
            case_mode = "smart_case",
        },
        ["ui-select"] = {},
    },
})

telescope.load_extension("fzf")
telescope.load_extension("file_browser")
telescope.load_extension("ui-select")
telescope.load_extension("gh")
telescope.load_extension("frecency")
-- telescope.load_extension("projects")
telescope.load_extension("refactoring")
