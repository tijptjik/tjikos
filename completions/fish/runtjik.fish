# Fish completion for runtjik.
source (status dirname)/__tjikos.fish
complete -c runtjik -f
complete -c runtjik -n '__tjikos_context exact' -a toggle -d Toggle
complete -c runtjik -n '__tjikos_context exact' -a show -d Show
complete -c runtjik -n '__tjikos_context exact' -a close -d Close
complete -c runtjik -n '__tjikos_context exact' -a quit -d Quit
complete -c runtjik -n '__tjikos_context exact' -a refresh -d Refresh
complete -c runtjik -n '__tjikos_context exact' -a stats -d Stats
complete -c runtjik -n '__tjikos_context exact' -a rebuild-counts -d 'Rebuild counts'
complete -c runtjik -n '__tjikos_context exact' -a check -d Check
complete -c runtjik -n '__tjikos_context exact' -a migrate-config -d 'Add missing configuration settings'
complete -c runtjik -n '__tjikos_context prefix migrate-config' -l enable-bundled-plugins -d 'Enable bundled plugins during migration'
complete -c runtjik -n '__tjikos_context exact' -a status -d Status
complete -c runtjik -n '__tjikos_context exact' -a version -d Version
complete -c runtjik -n '__tjikos_context exact' -l demo -d Demo
complete -c runtjik -n '__tjikos_context exact' -l version -d Version
