# Fish completion for bartjik.
source (status dirname)/__tjikos.fish
complete -c bartjik -f
complete -c bartjik -l help -d Help
complete -c bartjik -l config -d Config -r -F
complete -c bartjik -l daemon -d Daemon
complete -c bartjik -l demo -d Demo
