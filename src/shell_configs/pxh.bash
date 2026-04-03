preexec() {
    local cmd="$1"
    [ -z "$cmd" ] && return 1
    [[ "$cmd" =~ ^[[:space:]] ]] && return 1
    local started=$(date +%s)
    pxh \
	--db "$PXH_DB_PATH" \
	insert \
	--working-directory "$PWD" \
	--hostname "$PXH_HOSTNAME" \
	--shellname bash \
	--username "$USER" \
	--session-id "$PXH_SESSION_ID" \
	--start-unix-timestamp "$started" \
	-- "$cmd"
}

precmd() {
    local retval=$?
    local ended=$(date +%s)
    pxh \
	--db "$PXH_DB_PATH" \
	seal \
	--session-id "$PXH_SESSION_ID" \
	--end-unix-timestamp "$ended" \
	--exit-status "$retval"
}

_pxh_random() {
    od -An -N6 -tu8 < /dev/urandom | tr -d '\n '
}

__pxh_should_run=""

_pxh_recall() {
    __pxh_should_run=""
    local selected
    selected=$(pxh --db "$PXH_DB_PATH" recall --shell-mode --query "$READLINE_LINE" 2>/dev/null)
    if [[ "$selected" == run:* ]]; then
        READLINE_LINE="${selected#run:}"
        READLINE_POINT=${#READLINE_LINE}
        __pxh_should_run=1
    elif [[ "$selected" == edit-a:* ]]; then
        READLINE_LINE="${selected#edit-a:}"
        READLINE_POINT=0
    elif [[ "$selected" == edit:* ]]; then
        READLINE_LINE="${selected#edit:}"
        READLINE_POINT=${#READLINE_LINE}
    fi
}

_pxh_check_run() {
    if [[ "$__pxh_should_run" == 1 ]]; then
        __pxh_should_run=""
        bind '"\C-x3": accept-line'
    else
        bind '"\C-x3": ""'
    fi
}

_pxh_init() {
    export PXH_SESSION_ID=$(_pxh_random)
    export PXH_HOSTNAME=$(hostname -s)
    if [ -z "${PXH_DB_PATH:-}" ]; then
        local xdg_dir="${XDG_DATA_HOME:-$HOME/.local/share}/pxh"
        if [ -d "$xdg_dir" ]; then
            export PXH_DB_PATH="$xdg_dir/pxh.db"
        elif [ -d "$HOME/.pxh" ]; then
            export PXH_DB_PATH="$HOME/.pxh/pxh.db"
        else
            export PXH_DB_PATH="$xdg_dir/pxh.db"
        fi
    fi

    [ ! -d "$(dirname "$PXH_DB_PATH")" ] && mkdir -p -m 0700 "$(dirname "$PXH_DB_PATH")"

    # Bind Ctrl-R to pxh recall via macro chain: # PXH_CTRL_R_BINDING
    # \C-x1 runs recall, \C-x2 checks if we should execute, # PXH_CTRL_R_BINDING
    # \C-x3 is dynamically bound to accept-line or no-op # PXH_CTRL_R_BINDING
    bind -x '"\C-x1": _pxh_recall' # PXH_CTRL_R_BINDING
    bind -x '"\C-x2": _pxh_check_run' # PXH_CTRL_R_BINDING
    bind '"\C-x3": ""' # PXH_CTRL_R_BINDING
    bind '"\C-r": "\C-x1\C-x2\C-x3"' # PXH_CTRL_R_BINDING
}

_pxh_init
