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
    selected=$(pxh --db "$PXH_DB_PATH" recall 2>/dev/null)
    if [[ -n "$selected" ]]; then
        READLINE_LINE="$selected"
        READLINE_POINT=${#selected}
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
