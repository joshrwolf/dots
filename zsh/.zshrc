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
[[ -x /opt/homebrew/bin/python3.11 ]] && export CLOUDSDK_PYTHON='/opt/homebrew/bin/python3.11'

export CLAUDE_BASH_MAINTAIN_PROJECT_WORKING_DIR=1
export DISABLE_TELEMETRY=1
export BEADS_NO_DAEMON=1

# -----------------
# PATH
# -----------------

export PATH=$PATH:$HOME/.local/bin
[[ -d $HOME/.local/share/bob/nvim-bin ]] && export PATH=$PATH:$HOME/.local/share/bob/nvim-bin
(( $+commands[go] )) && { export GOPATH=$(go env GOPATH); export PATH=$PATH:$GOPATH/bin }
[[ -d $HOME/.cargo/bin ]] && export PATH=$PATH:$HOME/.cargo/bin
[[ -f $HOME/.cargo/env ]] && source $HOME/.cargo/env
[[ -d $HOME/.cw/bin ]] && export PATH="$HOME/.cw/bin:$PATH"
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
export FZF_DEFAULT_OPTS=$FZF_DEFAULT_OPTS'
  --color=fg:#656d76,fg+:#1F2328,bg:#ffffff,bg+:#deeeff
  --color=hl:#16a470,hl+:#953800,info:#9a6700,marker:#1a7f37
  --color=prompt:#0969da,spinner:#24292f,pointer:#8250df,header:#656d76
  --color=border:#d0d7de,label:#656d76,query:#1F2328
  --border="rounded" --border-label="" --preview-window="border-rounded" --prompt="> "
  --marker=">" --pointer="◆" --separator="─" --scrollbar="│"'
[ -f ~/.fzf.zsh ] && source ~/.fzf.zsh

[[ -f $HOME/.config/fzf/fzf-git.sh ]] && source $HOME/.config/fzf/fzf-git.sh
[[ -f $HOME/.config/fzf/fzf-git-extras.sh ]] && source $HOME/.config/fzf/fzf-git-extras.sh

# -----------------
# Functions
# -----------------

function imgsize() {
  crane manifest $1 --platform ${2:-linux/amd64} | jq '.config.size + ([.layers[].size] | add)' | numfmt --to=iec
}

# -----------------
# Git Worktree Detection
# -----------------

function _git_worktree_setup() {
  unset IN_GIT_WORKTREE GIT_WORKTREE_ROOT GIT_WORKTREE_NAME GIT_BARE_ROOT BEADS_DIR

  if command -v git >/dev/null 2>&1; then
    local git_dir=$(git rev-parse --git-dir 2>/dev/null)

    if [[ $? -eq 0 && -n "$git_dir" ]]; then
      if [[ "$git_dir" == *"/worktrees/"* ]]; then
        export IN_GIT_WORKTREE=1
        export GIT_WORKTREE_ROOT=$(git rev-parse --show-toplevel 2>/dev/null)
        export GIT_WORKTREE_NAME=$(basename "$GIT_WORKTREE_ROOT")
        export GIT_BARE_ROOT="${git_dir%/worktrees/*}"

        if [[ "$GIT_WORKTREE_NAME" != "main" ]]; then
          local main_beads="$(dirname "$GIT_WORKTREE_ROOT")/main/.beads"
          if [[ -d "$main_beads" ]]; then
            export BEADS_DIR="$main_beads"
          fi
        fi
      fi
    fi
  fi
}

_git_worktree_setup

# -----------------
# Local config
# -----------------

[[ -f ~/.zshrc.local ]] && source ~/.zshrc.local
