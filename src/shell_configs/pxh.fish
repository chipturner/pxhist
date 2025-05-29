function _pxh_preexec --on-event fish_preexec
    if test -n "$argv[1]"
        set started (date +%s)
        pxh \
            --db $PXH_DB_PATH \
            insert \
            --working-directory "$PWD" \
            --hostname "$PXH_HOSTNAME" \
            --shellname fish \
            --username "$USER" \
            --session-id $PXH_SESSION_ID \
            --start-unix-timestamp $started \
            "$argv[1]"
    end
end

function _pxh_postexec --on-event fish_postexec
    set retval $status
    set ended (date +%s)
    pxh \
        --db $PXH_DB_PATH \
        seal \
        --session-id $PXH_SESSION_ID \
        --end-unix-timestamp $ended \
        --exit-status $retval
end

function _pxh_random
    # Generate a random session ID using fish's random function
    echo (random 1000000000 9999999999)
end

function _pxh_init
    set -gx PXH_SESSION_ID (_pxh_random)
    set -gx PXH_HOSTNAME (hostname -s)
    set -gx PXH_DB_PATH $PXH_DB_PATH
    if test -z "$PXH_DB_PATH"
        set -gx PXH_DB_PATH "$HOME/.pxh/pxh.db"
    end

    set db_dir (dirname $PXH_DB_PATH)
    if not test -d $db_dir
        mkdir -p -m 0700 $db_dir
    end
end

_pxh_init