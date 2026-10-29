# Fish completion for bartjikctl.
source (status dirname)/__tjikos.fish
complete -c bartjikctl -f
complete -c bartjikctl -l help -d Help
complete -c bartjikctl -l config -d Config -r -F
complete -c bartjikctl -n '__tjikos_context exact' -a status -d Status
complete -c bartjikctl -n '__tjikos_context exact' -a monitors -d Monitors
complete -c bartjikctl -n '__tjikos_context exact' -a bars -d Bars
complete -c bartjikctl -n '__tjikos_context exact' -a modules -d Modules
complete -c bartjikctl -n '__tjikos_context exact' -a reload -d Reload
complete -c bartjikctl -n '__tjikos_context exact' -a show-workspaces -d 'Show workspaces'
complete -c bartjikctl -n '__tjikos_context exact' -a hide-workspaces -d 'Hide workspaces'
complete -c bartjikctl -n '__tjikos_context exact' -a spotify -d Spotify
complete -c bartjikctl -n '__tjikos_context exact' -a config -d Config
complete -c bartjikctl -n '__tjikos_context exact' -a show -d Show
complete -c bartjikctl -n '__tjikos_context exact' -a hide -d Hide
complete -c bartjikctl -n '__tjikos_context exact' -a toggle -d Toggle
complete -c bartjikctl -n '__tjikos_context exact' -a toggle-reveal -d 'Toggle reveal'
complete -c bartjikctl -n '__tjikos_context exact' -a show-reveal -d 'Show reveal'
complete -c bartjikctl -n '__tjikos_context exact' -a hide-reveal -d 'Hide reveal'
complete -c bartjikctl -n '__tjikos_context exact' -a init -d Init
complete -c bartjikctl -n '__tjikos_context exact spotify' -a login -d Login
complete -c bartjikctl -n '__tjikos_context exact spotify' -a logout -d Logout
complete -c bartjikctl -n '__tjikos_context exact spotify' -a status -d Status
complete -c bartjikctl -n '__tjikos_context prefix config' -l validate -d Validate
complete -c bartjikctl -n '__tjikos_context prefix init' -l preset -x -a 'desktop laptop minimal' -d Preset
complete -c bartjikctl -n '__tjikos_context prefix init' -l theme -x -a 'monochrome rose-punk' -d Theme
complete -c bartjikctl -n '__tjikos_context exact show' -a all -d All
complete -c bartjikctl -n '__tjikos_context exact hide' -a all -d All
complete -c bartjikctl -n '__tjikos_context exact toggle' -a all -d All
complete -c bartjikctl -n '__tjikos_context exact toggle-reveal' -a all -d All
complete -c bartjikctl -n '__tjikos_context exact show-reveal' -a all -d All
complete -c bartjikctl -n '__tjikos_context exact hide-reveal' -a all -d All
