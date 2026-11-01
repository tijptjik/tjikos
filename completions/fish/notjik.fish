# Fish completion for notjik.
source (status dirname)/__tjikos.fish
complete -c notjik -f
complete -c notjik -l help -d Help
complete -c notjik -l config -d Config -r -F
complete -c notjik -l daemon -d Daemon
complete -c notjik -n '__tjikos_context exact' -a demo -d 'Show synthetic notification cards'
complete -c notjik -n '__tjikos_context exact demo' -a 'mix list overview next clear' -d 'Notification demo'
complete -c notjik -n '__tjikos_context prefix demo' -l solo -d 'Use the normal Notjik service'
