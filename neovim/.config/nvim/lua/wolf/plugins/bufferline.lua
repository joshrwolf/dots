local M = {
	"akinsho/nvim-bufferline.lua",
	event = "BufReadPre",
}

function M.config()
	local severities = {
		"error",
		"warning",
	}

	require("bufferline").setup({
		options = {
			show_close_icon = false,
			color_icons = true,
			show_buffer_icons = true,
			separator_style = "thin",
            always_show_bufferline = false,
            offsets = {
                {
                    filetype = "neo-tree",
                    text = "Neo Tree",
                    highlight = "Directory",
                    text_align = "left",
                }
            }
		},
	})
end

return M
