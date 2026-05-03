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
            antimony,edit)
                cmd="antimony__subcmd__edit"
                ;;
            antimony,export)
                cmd="antimony__subcmd__export"
                ;;
            antimony,help)
                cmd="antimony__subcmd__help"
                ;;
            antimony,import)
                cmd="antimony__subcmd__import"
                ;;
            antimony,info)
                cmd="antimony__subcmd__info"
                ;;
            antimony,integrate)
                cmd="antimony__subcmd__integrate"
                ;;
            antimony,refresh)
                cmd="antimony__subcmd__refresh"
                ;;
            antimony,remove)
                cmd="antimony__subcmd__remove"
                ;;
            antimony,run)
                cmd="antimony__subcmd__run"
                ;;
            antimony,seccomp)
                cmd="antimony__subcmd__seccomp"
                ;;
            antimony__subcmd__help,edit)
                cmd="antimony__subcmd__help__subcmd__edit"
                ;;
            antimony__subcmd__help,export)
                cmd="antimony__subcmd__help__subcmd__export"
                ;;
            antimony__subcmd__help,help)
                cmd="antimony__subcmd__help__subcmd__help"
                ;;
            antimony__subcmd__help,import)
                cmd="antimony__subcmd__help__subcmd__import"
                ;;
            antimony__subcmd__help,info)
                cmd="antimony__subcmd__help__subcmd__info"
                ;;
            antimony__subcmd__help,integrate)
                cmd="antimony__subcmd__help__subcmd__integrate"
                ;;
            antimony__subcmd__help,refresh)
                cmd="antimony__subcmd__help__subcmd__refresh"
                ;;
            antimony__subcmd__help,remove)
                cmd="antimony__subcmd__help__subcmd__remove"
                ;;
            antimony__subcmd__help,run)
                cmd="antimony__subcmd__help__subcmd__run"
                ;;
            antimony__subcmd__help,seccomp)
                cmd="antimony__subcmd__help__subcmd__seccomp"
                ;;
            *)
                ;;
        esac
    done

    case "${cmd}" in
        antimony)
            opts="-h -V --help --version run edit refresh integrate remove seccomp export import info help"
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
        antimony__subcmd__edit)
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
        antimony__subcmd__export)
            opts="-h --name --dest --feature --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --name)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --dest)
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
        antimony__subcmd__help)
            opts="run edit refresh integrate remove seccomp export import info help"
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
        antimony__subcmd__help__subcmd__edit)
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
        antimony__subcmd__help__subcmd__export)
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
        antimony__subcmd__help__subcmd__help)
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
        antimony__subcmd__help__subcmd__import)
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
        antimony__subcmd__help__subcmd__info)
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
        antimony__subcmd__help__subcmd__integrate)
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
        antimony__subcmd__help__subcmd__refresh)
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
        antimony__subcmd__help__subcmd__remove)
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
        antimony__subcmd__help__subcmd__run)
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
        antimony__subcmd__help__subcmd__seccomp)
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
        antimony__subcmd__import)
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
        antimony__subcmd__info)
            opts="-h --feature --diff --help [NAME]"
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
        antimony__subcmd__integrate)
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
        antimony__subcmd__refresh)
            opts="-d -h --dry --hard --help [PROFILE] [PASSTHROUGH]..."
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
        antimony__subcmd__remove)
            opts="-h --feature --yes --help [NAME]"
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
        antimony__subcmd__run)
            opts="-d -r -l -c -h --dry --refresh --path --dir --lockdown --config --features --conflicts --inherits --home-policy --home-name --home-path --home-lock --seccomp --portals --see --talk --own --call --disable-ipc --system-bus --user-bus --file-passthrough --ro --rw --temp --binaries --libraries --directories --roots --no-sof --devices --namespaces --env --new-privileges --sandbox-args --help <PROFILE> [PASSTHROUGH]..."
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --path)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --dir)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --lockdown)
                    COMPREPLY=($(compgen -W "true false" -- "${cur}"))
                    return 0
                    ;;
                -l)
                    COMPREPLY=($(compgen -W "true false" -- "${cur}"))
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
                --temp)
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
                --directories)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --roots)
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
        antimony__subcmd__seccomp)
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
