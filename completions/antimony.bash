_antimony() {
    local i cur prev opts cmd
    COMPREPLY=()
    if [[ "${BASH_VERSINFO[0]}" -ge 4 ]]; then
        cur="$2"
    else
        cur="${COMP_WORDS[COMP_CWORD]}"
    fi
    prev="$3"
    cmd=""
    opts=""

    for i in "${COMP_WORDS[@]:0:COMP_CWORD}"
    do
        case "${cmd},${i}" in
            ",$1")
                cmd="antimony"
                ;;
            antimony,config)
                cmd="antimony__config"
                ;;
            antimony,edit)
                cmd="antimony__edit"
                ;;
            antimony,export)
                cmd="antimony__export"
                ;;
            antimony,help)
                cmd="antimony__help"
                ;;
            antimony,import)
                cmd="antimony__import"
                ;;
            antimony,info)
                cmd="antimony__info"
                ;;
            antimony,integrate)
                cmd="antimony__integrate"
                ;;
            antimony,refresh)
                cmd="antimony__refresh"
                ;;
            antimony,remove)
                cmd="antimony__remove"
                ;;
            antimony,run)
                cmd="antimony__run"
                ;;
            antimony,seccomp)
                cmd="antimony__seccomp"
                ;;
            antimony__help,config)
                cmd="antimony__help__config"
                ;;
            antimony__help,edit)
                cmd="antimony__help__edit"
                ;;
            antimony__help,export)
                cmd="antimony__help__export"
                ;;
            antimony__help,help)
                cmd="antimony__help__help"
                ;;
            antimony__help,import)
                cmd="antimony__help__import"
                ;;
            antimony__help,info)
                cmd="antimony__help__info"
                ;;
            antimony__help,integrate)
                cmd="antimony__help__integrate"
                ;;
            antimony__help,refresh)
                cmd="antimony__help__refresh"
                ;;
            antimony__help,remove)
                cmd="antimony__help__remove"
                ;;
            antimony__help,run)
                cmd="antimony__help__run"
                ;;
            antimony__help,seccomp)
                cmd="antimony__help__seccomp"
                ;;
            *)
                ;;
        esac
    done

    case "${cmd}" in
        antimony)
            opts="-h -V --help --version run edit refresh integrate remove info seccomp export import config help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 1 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        antimony__config)
            opts="-h --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        antimony__edit)
            opts="-h --feature --help <NAME>"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        antimony__export)
            opts="-h --feature --help [NAME] [DEST]"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        antimony__help)
            opts="run edit refresh integrate remove info seccomp export import config help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        antimony__help__config)
            opts=""
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        antimony__help__edit)
            opts=""
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        antimony__help__export)
            opts=""
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        antimony__help__help)
            opts=""
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        antimony__help__import)
            opts=""
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        antimony__help__info)
            opts=""
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        antimony__help__integrate)
            opts=""
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        antimony__help__refresh)
            opts=""
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        antimony__help__remove)
            opts=""
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        antimony__help__run)
            opts=""
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        antimony__help__seccomp)
            opts=""
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        antimony__import)
            opts="-h --help <PROFILE>"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        antimony__info)
            opts="-v -h --verbosity --help profile feature seccomp [NAME]"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        antimony__integrate)
            opts="-r -s -c -h --remove --shadow --config-mode --create-desktop --help <PROFILE>"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --config-mode)
                    COMPREPLY=($(compgen -W "action file" -- "${cur}"))
                    return 0
                    ;;
                -c)
                    COMPREPLY=($(compgen -W "action file" -- "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        antimony__refresh)
            opts="-d -i -h --dry --hard --integrate --help [PROFILE] [PASSTHROUGH]..."
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        antimony__remove)
            opts="-h --feature --help [NAME]"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        antimony__run)
            opts="-d -l -r -c -h --dry --log --refresh --path --config --features --conflicts --inherits --home-policy --home-name --home-path --home-lock --seccomp --portals --see --talk --own --call --disable-ipc --system-bus --user-bus --file-passthrough --ro --rw --binaries --libraries --devices --namespaces --env --new-privileges --sandbox-args --help <PROFILE> [PASSTHROUGH]..."
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --path)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --config)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                -c)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --features)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --conflicts)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --inherits)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --home-policy)
                    COMPREPLY=($(compgen -W "none enabled read-only overlay" -- "${cur}"))
                    return 0
                    ;;
                --home-name)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --home-path)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --home-lock)
                    COMPREPLY=($(compgen -W "true false" -- "${cur}"))
                    return 0
                    ;;
                --seccomp)
                    COMPREPLY=($(compgen -W "disabled permissive enforcing notifying" -- "${cur}"))
                    return 0
                    ;;
                --portals)
                    COMPREPLY=($(compgen -W "background camera clipboard documents file-chooser global-shortcuts inhibit location notifications open-uri proxy-resolver realtime screen-cast screenshot settings secret network-monitor" -- "${cur}"))
                    return 0
                    ;;
                --see)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --talk)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --own)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --call)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --file-passthrough)
                    COMPREPLY=($(compgen -W "read-only read-write executable" -- "${cur}"))
                    return 0
                    ;;
                --ro)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --rw)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --binaries)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --libraries)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --devices)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --namespaces)
                    COMPREPLY=($(compgen -W "all user ipc pid net uts c-group" -- "${cur}"))
                    return 0
                    ;;
                --env)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --new-privileges)
                    COMPREPLY=($(compgen -W "true false" -- "${cur}"))
                    return 0
                    ;;
                --sandbox-args)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        antimony__seccomp)
            opts="-h --help optimize remove export merge clean [PATH]"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
    esac
}

if [[ "${BASH_VERSINFO[0]}" -eq 4 && "${BASH_VERSINFO[1]}" -ge 4 || "${BASH_VERSINFO[0]}" -gt 4 ]]; then
    complete -F _antimony -o nosort -o bashdefault -o default antimony
else
    complete -F _antimony -o bashdefault -o default antimony
fi
