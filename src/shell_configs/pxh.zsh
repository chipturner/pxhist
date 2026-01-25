_pxh_addhistory() {
    local cmd="${1[0, -2]}"
    [ -z "$cmd" ] && return 1
    [[ "$cmd" =~ ^[[:space:]] ]] && return 1
    local started=$EPOCHSECONDS
    pxh \
	--db $PXH_DB_PATH \
	insert \
	--working-directory "$PWD" \
	--hostname "$PXH_HOSTNAME" \
	--shellname zsh \
	--username "$USER" \
	--session-id $PXH_SESSION_ID \
	--start-unix-timestamp $started \
	"$cmd"
}

_pxh_update_last_status() {
    local retval=$?
    local ended=$EPOCHSECONDS
    pxh \
	--db $PXH_DB_PATH \
	seal \
	--session-id $PXH_SESSION_ID \
	--end-unix-timestamp $ended \
	--exit-status $retval
}

_pxh_random() {
    zmodload zsh/mathfunc
    print $(( int(rand48() * 1 << 48) ))
}

_pxh_recall_widget() {
    local selected
    selected=$(pxh --db "$PXH_DB_PATH" recall --query "$BUFFER" 2>&1)
    if [[ "$selected" == run:* ]]; then
        # Execute immediately
        BUFFER="${selected#run:}"
        zle accept-line
    elif [[ "$selected" == edit:* ]]; then
        # Place in buffer for editing
        BUFFER="${selected#edit:}"
        CURSOR=${#BUFFER}
        zle reset-prompt
    fi
}

_pxh_init() {
    PXH_SESSION_ID=$(_pxh_random)
    PXH_HOSTNAME=$(hostname -s)
    export PXH_DB_PATH=${PXH_DB_PATH:-$HOME/.pxh/pxh.db}

    [ ! -d $(dirname $PXH_DB_PATH) ] && mkdir -p -m 0700 $(dirname $PXH_DB_PATH)

    zmodload zsh/datetime # epochseconds
    autoload -Uz add-zsh-hook
    add-zsh-hook zshaddhistory _pxh_addhistory
    add-zsh-hook precmd _pxh_update_last_status

    # Bind Ctrl-R to pxh recall # PXH_CTRL_R_BINDING
    zle -N _pxh_recall_widget # PXH_CTRL_R_BINDING
    bindkey '^R' _pxh_recall_widget # PXH_CTRL_R_BINDING
}

_pxh_init
