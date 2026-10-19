# Zephyr's ls, completion, and man colors inherit Ghostty's ANSI palette.
# Explicit syntax colors use GitHub Light's code palette; suggestions stay muted.
ZSH_AUTOSUGGEST_HIGHLIGHT_STYLE='fg=#6e7781'
typeset -gA ZSH_HIGHLIGHT_STYLES
ZSH_HIGHLIGHT_STYLES[comment]='fg=#6e7781'
ZSH_HIGHLIGHT_STYLES[unknown-token]='fg=#cf222e,underline'
ZSH_HIGHLIGHT_STYLES[reserved-word]='fg=#cf222e'
ZSH_HIGHLIGHT_STYLES[arg0]='fg=#8250df'
ZSH_HIGHLIGHT_STYLES[single-quoted-argument]='fg=#0a3069'
ZSH_HIGHLIGHT_STYLES[double-quoted-argument]='fg=#0a3069'
ZSH_HIGHLIGHT_STYLES[dollar-quoted-argument]='fg=#0a3069'
