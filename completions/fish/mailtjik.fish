# Fish completion for mailtjik.
source (status dirname)/__tjikos.fish
complete -c mailtjik -f
complete -c mailtjik -n '__tjikos_context exact' -a read -d 'Read archived mail'
complete -c mailtjik -n '__tjikos_context exact' -a archive -d 'Manage mail archives'
complete -c mailtjik -l config -d Config -r -F
complete -c mailtjik -l demo -d Demo
complete -c mailtjik -l help -d Help
