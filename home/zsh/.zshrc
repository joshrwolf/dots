# The machine environment lives in .zprofile, which only login shells read.
[[ -o login ]] || source "${ZDOTDIR:-$HOME}/.zprofile"

export EDITOR="nvim"
export VISUAL="$EDITOR"
export GITSIGN_CONNECTOR_ID="https://accounts.google.com"

alias vi='nvim'
alias vim='nvim'

# Keep the Git shortcuts explicit; Zephyr does not provide a Git alias module.
alias g='git'
alias G='git'
alias gst='git status'

# App palettes follow the shared theme selector, like Neovim.
export STARSHIP_CONFIG="${XDG_CONFIG_HOME:-$HOME/.config}/theme/current/starship.toml"
export FZF_DEFAULT_OPTS_FILE="${XDG_CONFIG_HOME:-$HOME/.config}/theme/current/fzf"
export EZA_CONFIG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/theme/current/eza"

# Zephyr settings must be defined before Antidote loads its modules.
zstyle ':zephyr:plugin:completion' use-cache yes
zstyle ':zephyr:plugin:completion' immediate yes
zstyle ':zephyr:plugin:completion' compstyle zephyr
zstyle ':zephyr:plugin:editor' key-bindings emacs
zstyle ':zephyr:plugin:prompt' theme starship
zstyle ':zephyr:plugin:prompt' use-cache yes
zstyle ':zephyr:plugin:prompt' immediate yes

fzf-shell-init() {
  (( $+commands[fzf] )) || return 0
  source <(fzf --zsh)
}

source "$HOME/.local/share/antidote/antidote.zsh"
antidote load "${ZDOTDIR:-$HOME}/.zsh_plugins.txt"

# Cloud SDK's completion script runs after compinit.
if [[ -r "$CLOUDSDK_ROOT_DIR/completion.zsh.inc" ]]; then
  source "$CLOUDSDK_ROOT_DIR/completion.zsh.inc"
fi

# Zephyr has no eza module. Replace its BSD ls alias after loading color.
if (( $+commands[eza] )); then
  alias ls='eza --icons=auto --group-directories-first --no-git'
  alias ll='ls --long'
  alias l='ll'
  alias la='ll --all'
  alias lt='ls --tree --level=2'
fi

# Apply after plugins so their default styles cannot overwrite the selected theme.
[[ ! -r "${XDG_CONFIG_HOME:-$HOME/.config}/theme/current/zsh.zsh" ]] ||
  source "${XDG_CONFIG_HOME:-$HOME/.config}/theme/current/zsh.zsh"

# Include dotfiles in tab completion without changing ordinary glob expansion.
_comp_options+=(globdots)

if (( $+commands[zoxide] )); then
  eval "$(zoxide init zsh)"
fi

# Keep directory-scoped tool selection active as the shell moves.
if (( $+commands[mise] )); then
  eval "$(mise activate zsh)"
fi

# Rex shell integration: prompt marks and long-command status. Last, so its
# hooks run before every other plugin's.
[[ -z $REX_BLOCK ]] || source "${XDG_CONFIG_HOME:-$HOME/.config}/zsh/rex.zsh"
