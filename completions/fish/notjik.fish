# Fish completion for notjik.
source (status dirname)/__tjikos.fish
complete -c notjik -f
complete -c notjik -l help -d Help
complete -c notjik -l config -d Config -r -F
complete -c notjik -l daemon -d Daemon
