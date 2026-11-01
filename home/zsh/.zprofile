typeset -U path PATH
path=("$HOME/.local/bin" $path)

# OrbStack owns its optional command-line integration.
[[ ! -r "$HOME/.orbstack/shell/init.zsh" ]] || source "$HOME/.orbstack/shell/init.zsh"

# Load the selected machine environment before interactive plugins need it.
if (( $+commands[mise] )); then
  eval "$(mise env --shell zsh)"
  eval "$(mise activate zsh --shims)"
fi
