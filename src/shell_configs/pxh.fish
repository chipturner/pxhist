function _pxh_preexec --on-event fish_preexec
    set -l cmd $argv[1]
    if test -z "$cmd"
        return 1
    end
    set -l started (date +%s)
    pxh \
        --db $PXH_DB_PATH \
        insert \
        --working-directory $PWD \
        --hostname $PXH_HOSTNAME \
        --shellname fish \
        --username $USER \
        --session-id $PXH_SESSION_ID \
        --start-unix-timestamp $started \
        $cmd
end

function _pxh_postexec --on-event fish_postexec
    set -l retval $status
    set -l ended (date +%s)
    pxh \
        --db $PXH_DB_PATH \
        seal \
        --session-id $PXH_SESSION_ID \
        --end-unix-timestamp $ended \
        --exit-status $retval
end

function _pxh_random
    # Generate a random session ID using fish's random function
    echo (random)(random)(random)(random)
end

function _pxh_init
    set -gx PXH_SESSION_ID (_pxh_random)
    set -gx PXH_HOSTNAME (hostname -s)
    set -gx PXH_DB_PATH (test -n "$PXH_DB_PATH"; and echo $PXH_DB_PATH; or echo $HOME/.pxh/pxh.db)

    if not test -d (dirname $PXH_DB_PATH)
        mkdir -p -m 0700 (dirname $PXH_DB_PATH)
    end
end

_pxh_init