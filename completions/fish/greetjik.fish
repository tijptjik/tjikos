# Fish completion for greetjik.
source (status dirname)/__tjikos.fish
complete -c greetjik -f
complete -c greetjik -s h -l help -d 'Show help'
complete -c greetjik -s V -l version -d 'Show version'
complete -c greetjik -s l -l list -d 'List pony names'
complete -c greetjik -s a -l all -d 'Include extra ponies'
complete -c greetjik -s A -l extra -d 'Use extra ponies only'
complete -c greetjik -l tty -d 'Use TTY artwork'
complete -c greetjik -s W -l width -x -d 'Quote width in columns'
complete -c greetjik -s f -l pony -x -a '(command greetjik --all --list 2>/dev/null)' -d 'Pony name'
