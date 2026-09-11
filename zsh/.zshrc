# -----------------
# Zsh configuration
# -----------------

# History
setopt HIST_IGNORE_ALL_DUPS

# Input/output
bindkey -e

# Remove path separator from WORDCHARS.
WORDCHARS=${WORDCHARS//[\/]}

# -----------------
# Zim configuration
# -----------------

# Module configuration
ZSH_AUTOSUGGEST_MANUAL_REBIND=1
ZSH_HIGHLIGHT_HIGHLIGHTERS=(main brackets)

# ------------------
# Initialize modules
# ------------------

ZIM_HOME=${ZDOTDIR:-${HOME}}/.zim
if [[ ! -e ${ZIM_HOME}/zimfw.zsh ]]; then
  if (( ${+commands[curl]} )); then
    curl -fsSL --create-dirs -o ${ZIM_HOME}/zimfw.zsh \
        https://github.com/zimfw/zimfw/releases/latest/download/zimfw.zsh
  else
    mkdir -p ${ZIM_HOME} && wget -nv -O ${ZIM_HOME}/zimfw.zsh \
        https://github.com/zimfw/zimfw/releases/latest/download/zimfw.zsh
  fi
fi
if [[ ! ${ZIM_HOME}/init.zsh -nt ${ZDOTDIR:-${HOME}}/.zimrc ]]; then
  source ${ZIM_HOME}/zimfw.zsh init -q
fi
source ${ZIM_HOME}/init.zsh

# ------------------------------
# Post-init module configuration
# ------------------------------

# zim's environment module sets NO_CLOBBER; allow > to overwrite
setopt CLOBBER

# zsh-history-substring-search
zmodload -F zsh/terminfo +p:terminfo
for key ('^[[A' '^P' ${terminfo[kcuu1]}) bindkey ${key} history-substring-search-up
for key ('^[[B' '^N' ${terminfo[kcud1]}) bindkey ${key} history-substring-search-down
for key ('k') bindkey -M vicmd ${key} history-substring-search-up
for key ('j') bindkey -M vicmd ${key} history-substring-search-down
unset key

# -----------------
# Environment
# -----------------

export BAT_THEME="GitHub"
export EDITOR="nvim"
export VISUAL="nvim"
export CLOUDSDK_PYTHON='/opt/homebrew/bin/python3'

export CLAUDE_BASH_MAINTAIN_PROJECT_WORKING_DIR=1

# -----------------
# PATH
# -----------------

export PATH=$PATH:$HOME/.local/bin
[[ -d $HOME/.local/share/bob/nvim-bin ]] && export PATH=$PATH:$HOME/.local/share/bob/nvim-bin
(( $+commands[go] )) && { export GOPATH=$(go env GOPATH); export PATH=$PATH:$GOPATH/bin }
[[ -d $HOME/.cargo/bin ]] && export PATH=$PATH:$HOME/.cargo/bin
[[ -f $HOME/.cargo/env ]] && source $HOME/.cargo/env
[[ -d /opt/homebrew/opt/gnu-getopt/bin ]] && export PATH="/opt/homebrew/opt/gnu-getopt/bin:$PATH"

# -----------------
# Aliases
# -----------------

alias vim="nvim"
alias vi="nvim"
alias k="kubectl"
alias cat="bat -p"
alias gst="git status"
alias tf=terraform
# A plain Codex launch can reuse an app-server with a stale environment. Passing
# a session override keeps shell tools tied to the environment of this shell.
alias codex='codex -c shell_environment_policy.inherit=all'

# -----------------
# Completions (macOS/Homebrew)
# -----------------

if (( $+commands[brew] )); then
  local brew_prefix=$(brew --prefix)
  for f in _kustomize _kind _gh; do
    [[ -f $brew_prefix/share/zsh/site-functions/$f ]] && source "$brew_prefix/share/zsh/site-functions/$f"
  done
  [[ -f $brew_prefix/share/google-cloud-sdk/path.zsh.inc ]] && source "$brew_prefix/share/google-cloud-sdk/path.zsh.inc"
  [[ -f $brew_prefix/Caskroom/google-cloud-sdk/latest/google-cloud-sdk/completion.zsh.inc ]] && source "$brew_prefix/Caskroom/google-cloud-sdk/latest/google-cloud-sdk/completion.zsh.inc"
  [[ -f $brew_prefix/share/zsh/site-functions/aws_zsh_completer.sh ]] && source "$brew_prefix/share/zsh/site-functions/aws_zsh_completer.sh"
fi

# -----------------
# FZF
# -----------------

export FZF_DEFAULT_OPTS='--bind ctrl-u:preview-page-up,ctrl-d:preview-page-down'
# GitHub Light. fg is the primary text colour rather than the muted one: fzf
# paints every unstyled line with fg, so a muted fg makes a whole list read as
# secondary and leaves nothing for a producer to de-emphasise against. Anything
# that should recede says so with its own escape.
# bg is -1 so the terminal's own background shows through, and gutter is -1 so
# fzf stops drawing a tinted rail down the left of every row.
export FZF_DEFAULT_OPTS=$FZF_DEFAULT_OPTS'
  --color=fg:#1f2328,fg+:#1f2328,bg:-1,bg+:#deeeff,gutter:-1
  --color=hl:#953800,hl+:#953800,info:#8c959f,marker:#1a7f37
  --color=prompt:#0969da,spinner:#1a7f37,pointer:#8250df,header:#8c959f
  --color=border:#d0d7de,label:#8c959f,query:#1f2328
  --border="rounded" --border-label="" --preview-window="border-rounded" --prompt="> "
  --marker=">" --pointer="◆" --separator="─" --scrollbar="│"'
[ -f ~/.fzf.zsh ] && source ~/.fzf.zsh

[[ -f $HOME/.config/fzf/fzf-git.sh ]] && source $HOME/.config/fzf/fzf-git.sh
# -----------------
# Functions
# -----------------

function imgsize() {
  crane manifest $1 --platform ${2:-linux/amd64} | jq '.config.size + ([.layers[].size] | add)' | numfmt --to=iec
}

# -----------------
# Worktrunk
# -----------------

if command -v wt >/dev/null 2>&1; then
  eval "$(command wt config shell init zsh)"
fi

# -----------------
# Local config
# -----------------

[[ -f ~/.zshrc.local ]] && source ~/.zshrc.local
export PATH="/opt/homebrew/opt/e2fsprogs/bin:$PATH"
export PATH="/opt/homebrew/opt/e2fsprogs/sbin:$PATH"
