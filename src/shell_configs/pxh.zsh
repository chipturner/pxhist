_pxh_addhistory() {
    local cmd="${1[0, -2]}"
    [ -z "$cmd" ] && return 1
    [[ "$cmd" =~ ^[[:space:]] ]] && return 1
    local started=$EPOCHSECONDS
    pxh \
	--db "$PXH_DB_PATH" \
	insert \
	--working-directory "$PWD" \
	--hostname "$PXH_HOSTNAME" \
	--shellname zsh \
	--username "$USER" \
	--session-id "$PXH_SESSION_ID" \
	--start-unix-timestamp "$started" \
	"$cmd"
}

_pxh_update_last_status() {
    local retval=$?
    local ended=$EPOCHSECONDS
    pxh \
	--db "$PXH_DB_PATH" \
	seal \
	--session-id "$PXH_SESSION_ID" \
	--end-unix-timestamp "$ended" \
	--exit-status "$retval"
}

_pxh_random() {
    zmodload zsh/mathfunc
    print $(( int(rand48() * 1 << 48) ))
}

_pxh_recall_widget() {
    local selected
    selected=$(pxh --db "$PXH_DB_PATH" recall --shell-mode --query "$BUFFER" 2>&1)
    if [[ "$selected" == run:* ]]; then
        # Execute immediately
        BUFFER="${selected#run:}"
        zle accept-line
    elif [[ "$selected" == edit-a:* ]]; then
        # Place in buffer for editing, cursor at beginning
        BUFFER="${selected#edit-a:}"
        CURSOR=0
        zle reset-prompt
    elif [[ "$selected" == edit:* ]]; then
        # Place in buffer for editing, cursor at end
        BUFFER="${selected#edit:}"
        CURSOR=${#BUFFER}
        zle reset-prompt
    fi
}

_zsh_autosuggest_strategy_pxh() {
    typeset -g suggestion
    suggestion=$(pxh --db "$PXH_DB_PATH" autosuggest -- "$1" 2>/dev/null)
}

_pxh_init() {
    PXH_SESSION_ID=$(_pxh_random)
    PXH_HOSTNAME=$(hostname -s)
    if [ -z "${PXH_DB_PATH:-}" ]; then
        if [ -d "$HOME/.pxh" ]; then
            export PXH_DB_PATH="$HOME/.pxh/pxh.db"
        else
            export PXH_DB_PATH="${XDG_DATA_HOME:-$HOME/.local/share}/pxh/pxh.db"
        fi
    fi

    [ ! -d "$(dirname "$PXH_DB_PATH")" ] && mkdir -p -m 0700 "$(dirname "$PXH_DB_PATH")"

    zmodload zsh/datetime # epochseconds
    autoload -Uz add-zsh-hook
    add-zsh-hook zshaddhistory _pxh_addhistory
    add-zsh-hook precmd _pxh_update_last_status

    if [[ -n "${ZSH_AUTOSUGGEST_STRATEGY:-}" ]]; then
        ZSH_AUTOSUGGEST_STRATEGY=(pxh $ZSH_AUTOSUGGEST_STRATEGY)
    else
        ZSH_AUTOSUGGEST_STRATEGY=(pxh history)
    fi

    # Bind Ctrl-R to pxh recall # PXH_CTRL_R_BINDING
    zle -N _pxh_recall_widget # PXH_CTRL_R_BINDING
    bindkey '^R' _pxh_recall_widget # PXH_CTRL_R_BINDING
}

_pxh_init
