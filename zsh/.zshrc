typeset -U path PATH

path=(
  "$HOME/.local/share/bob/nvim-bin"
  "$HOME/.local/bin"
  # Use GNU command names for portable Linux-oriented build scripts.
  "${HOMEBREW_PREFIX:-/opt/homebrew}/opt/coreutils/libexec/gnubin"
  "${HOMEBREW_PREFIX:-/opt/homebrew}/opt/gnu-tar/libexec/gnubin"
  "${HOMEBREW_PREFIX:-/opt/homebrew}/opt/make/libexec/gnubin"
  # Homebrew keeps unversioned python/pip here; opt/python follows its default release.
  "${HOMEBREW_PREFIX:-/opt/homebrew}/opt/python/libexec/bin"
  "${HOMEBREW_PREFIX:-/opt/homebrew}/bin"
  # Include optional components installed through gcloud's component manager.
  "${HOMEBREW_PREFIX:-/opt/homebrew}/share/google-cloud-sdk/bin"
  "${HOMEBREW_PREFIX:-/opt/homebrew}/opt/rustup/bin"
  $path
)

export EDITOR="nvim"
export VISUAL="$EDITOR"

alias vi='nvim'
alias vim='nvim'

# Keep the Git shortcuts explicit; Zephyr does not provide a Git alias module.
alias g='git'
alias G='git'
alias gst='git status'

# App palettes follow the same selector as Ghostty and Neovim.
export STARSHIP_CONFIG="${XDG_CONFIG_HOME:-$HOME/.config}/theme/current/starship.toml"
export FZF_DEFAULT_OPTS_FILE="${XDG_CONFIG_HOME:-$HOME/.config}/theme/current/fzf"
export EZA_CONFIG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/theme/current/eza"

# Zephyr settings must be defined before Antidote loads its modules.
zstyle ':zephyr:plugin:homebrew' use-cache yes
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

source "${HOMEBREW_PREFIX:-/opt/homebrew}/opt/antidote/share/antidote/antidote.zsh"
antidote load "${ZDOTDIR:-$HOME}/.zsh_plugins.txt"

# Google's completion script must run after compinit; the cask's symlink alone isn't autoloadable.
if [[ -r "${HOMEBREW_PREFIX:-/opt/homebrew}/share/google-cloud-sdk/completion.zsh.inc" ]]; then
  source "${HOMEBREW_PREFIX:-/opt/homebrew}/share/google-cloud-sdk/completion.zsh.inc"
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
