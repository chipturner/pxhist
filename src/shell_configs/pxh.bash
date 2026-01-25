preexec() {
    local cmd="$1"
    [ -z "$cmd" ] && return 1
    [[ "$cmd" =~ ^[[:space:]] ]] && return 1
    local started=$(date +%s)
    pxh \
	--db $PXH_DB_PATH \
	insert \
	--working-directory "$PWD" \
	--hostname "$PXH_HOSTNAME" \
	--shellname bash \
	--username "$USER" \
	--session-id $PXH_SESSION_ID \
	--start-unix-timestamp $started \
	"$cmd"
}

precmd() {
    local retval=$?
    local ended=$(date +%s)
    pxh \
	--db $PXH_DB_PATH \
	seal \
	--session-id $PXH_SESSION_ID \
	--end-unix-timestamp $ended \
	--exit-status $retval
}

_pxh_random() {
    od -An -N6 -tu8 < /dev/urandom | tr -d '\n '
}

_pxh_recall() {
    local selected
    selected=$(pxh --db "$PXH_DB_PATH" recall --query "$READLINE_LINE" 2>/dev/null)
    if [[ "$selected" == run:* ]]; then
        # Execute immediately
        READLINE_LINE="${selected#run:}"
        READLINE_POINT=${#READLINE_LINE}
        # Simulate pressing Enter by accepting the line
        # Note: bind -x functions can't directly execute, so we rely on
        # the user pressing Enter or we could use READLINE_LINE and accept-line
    elif [[ "$selected" == edit:* ]]; then
        # Place in buffer for editing
        READLINE_LINE="${selected#edit:}"
        READLINE_POINT=${#READLINE_LINE}
    fi
}

_pxh_init() {
    PXH_SESSION_ID=$(_pxh_random)
    PXH_HOSTNAME=$(hostname -s)
    export PXH_DB_PATH=${PXH_DB_PATH:-$HOME/.pxh/pxh.db}

    [ ! -d $(dirname $PXH_DB_PATH) ] && mkdir -p -m 0700 $(dirname $PXH_DB_PATH)

    # Bind Ctrl-R to pxh recall
    bind -x '"\C-r": _pxh_recall'
}

_pxh_init
