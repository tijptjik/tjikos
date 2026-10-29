# Fish completion for notjikctl.
source (status dirname)/__tjikos.fish
complete -c notjikctl -f
complete -c notjikctl -l help -d Help
complete -c notjikctl -n '__tjikos_context exact' -a pause -d Pause
complete -c notjikctl -n '__tjikos_context exact' -a resume -d Resume
complete -c notjikctl -n '__tjikos_context exact' -a toggle -d Toggle
complete -c notjikctl -n '__tjikos_context exact' -a status -d Status
complete -c notjikctl -n '__tjikos_context exact' -a list -d List
complete -c notjikctl -n '__tjikos_context exact' -a monitors -d Monitors
complete -c notjikctl -n '__tjikos_context exact' -a reload -d Reload
complete -c notjikctl -n '__tjikos_context exact' -a send -d Send
complete -c notjikctl -n '__tjikos_context exact' -a dismiss -d Dismiss
complete -c notjikctl -n '__tjikos_context exact' -a action -d Action
complete -c notjikctl -n '__tjikos_context exact' -a history -d History
complete -c notjikctl -n '__tjikos_context exact' -a navigate -d Navigate
complete -c notjikctl -n '__tjikos_context exact' -a unfocus -d Unfocus
complete -c notjikctl -n '__tjikos_context exact' -a logs -d Logs
complete -c notjikctl -n '__tjikos_context exact' -a init -d Init
complete -c notjikctl -n '__tjikos_context exact' -a config -d Config
complete -c notjikctl -n '__tjikos_context exact' -a demo -d Demo
complete -c notjikctl -n '__tjikos_context exact' -a compare -d Compare
complete -c notjikctl -n '__tjikos_context exact' -a telegram -d Telegram
complete -c notjikctl -n '__tjikos_context prefix send' -l app -d App -r -f
complete -c notjikctl -n '__tjikos_context prefix send' -l timeout -d Timeout -r -f
complete -c notjikctl -n '__tjikos_context prefix send' -l icon -d Icon -r -f
complete -c notjikctl -n '__tjikos_context prefix send' -l image -d Image -r -F
complete -c notjikctl -n '__tjikos_context prefix send' -l replace -d Replace -r -f
complete -c notjikctl -n '__tjikos_context prefix send' -l id -d Id -r -f
complete -c notjikctl -n '__tjikos_context prefix send' -l group -d Group -r -f
complete -c notjikctl -n '__tjikos_context prefix send' -l data -d Data -r -f
complete -c notjikctl -n '__tjikos_context prefix send' -l actions -d Actions -r -f
complete -c notjikctl -n '__tjikos_context prefix send' -l resident -d Resident
complete -c notjikctl -n '__tjikos_context prefix send' -l transient -d Transient
complete -c notjikctl -n '__tjikos_context prefix send' -l urgency -x -a 'low normal critical' -d Urgency
complete -c notjikctl -n '__tjikos_context prefix send' -l type -x -a 'standard progress status timer telegram results-table' -d Type
complete -c notjikctl -n '__tjikos_context exact dismiss' -a all -d All
complete -c notjikctl -n '__tjikos_context exact history' -a show -d Show
complete -c notjikctl -n '__tjikos_context exact history' -a hide -d Hide
complete -c notjikctl -n '__tjikos_context exact history' -a toggle -d Toggle
complete -c notjikctl -n '__tjikos_context exact navigate' -a previous -d Previous
complete -c notjikctl -n '__tjikos_context exact navigate' -a next -d Next
complete -c notjikctl -n '__tjikos_context prefix logs' -l limit -d Limit -r -f
complete -c notjikctl -n '__tjikos_context prefix logs' -l json -d Json
complete -c notjikctl -n '__tjikos_context prefix logs' -l follow -d Follow
complete -c notjikctl -n '__tjikos_context prefix logs' -l app -d App -r -f
complete -c notjikctl -n '__tjikos_context prefix logs' -l event -d Event -r -f
complete -c notjikctl -n '__tjikos_context prefix logs' -l since -d Since -r -f
complete -c notjikctl -n '__tjikos_context prefix init' -l theme -x -a 'monochrome rose-punk' -d Theme
complete -c notjikctl -n '__tjikos_context exact telegram' -a setup -d Setup
complete -c notjikctl -n '__tjikos_context exact telegram' -a login -d Login
complete -c notjikctl -n '__tjikos_context exact telegram' -a run -d Run
complete -c notjikctl -n '__tjikos_context exact telegram' -a status -d Status
complete -c notjikctl -n '__tjikos_context exact telegram' -a action -d Action
complete -c notjikctl -n '__tjikos_context exact telegram' -a doctor -d Doctor
complete -c notjikctl -n '__tjikos_context exact compare' -a start -d Start
complete -c notjikctl -n '__tjikos_context exact compare' -a stop -d Stop
complete -c notjikctl -n '__tjikos_context exact compare' -a status -d Status
complete -c notjikctl -n '__tjikos_context exact compare' -a clear -d Clear
complete -c notjikctl -n '__tjikos_context exact compare' -a cases -d Cases
complete -c notjikctl -n '__tjikos_context exact compare' -a send -d Send
complete -c notjikctl -n '__tjikos_context exact compare' -a case -d Case
complete -c notjikctl -n '__tjikos_context prefix compare start' -l dunst -d Dunst -r -F
complete -c notjikctl -n '__tjikos_context prefix demo' -l solo -d Solo
complete -c notjikctl -n '__tjikos_context prefix demo' -l append -d Append
complete -c notjikctl -n '__tjikos_context prefix demo' -l interval -d Interval -r -f
complete -c notjikctl -n '__tjikos_context prefix demo' -l timeout -d Timeout -r -f
complete -c notjikctl -n '__tjikos_context prefix demo' -l log -d Log
complete -c notjikctl -n '__tjikos_context exact demo' -a list -d List
complete -c notjikctl -n '__tjikos_context exact demo' -a overview -d Overview
complete -c notjikctl -n '__tjikos_context exact demo' -a all -d All
complete -c notjikctl -n '__tjikos_context exact demo' -a robots -d Robots
complete -c notjikctl -n '__tjikos_context exact demo' -a next -d Next
complete -c notjikctl -n '__tjikos_context exact demo' -a clear -d Clear
complete -c notjikctl -n '__tjikos_context exact demo' -a standard -d Standard
complete -c notjikctl -n '__tjikos_context exact demo' -a progress -d Progress
complete -c notjikctl -n '__tjikos_context exact demo' -a status -d Status
complete -c notjikctl -n '__tjikos_context exact demo' -a timer -d Timer
complete -c notjikctl -n '__tjikos_context exact demo' -a telegram -d Telegram
complete -c notjikctl -n '__tjikos_context exact demo' -a results-table -d 'Results table'
complete -c notjikctl -n '__tjikos_context prefix demo' -a title-only -d 'Notification fixture'
complete -c notjikctl -n '__tjikos_context prefix demo' -a icon -d 'Notification fixture'
complete -c notjikctl -n '__tjikos_context prefix demo' -a missing-icon -d 'Notification fixture'
complete -c notjikctl -n '__tjikos_context prefix demo' -a image -d 'Notification fixture'
complete -c notjikctl -n '__tjikos_context prefix demo' -a progress-empty -d 'Notification fixture'
complete -c notjikctl -n '__tjikos_context prefix demo' -a progress-complete -d 'Notification fixture'
complete -c notjikctl -n '__tjikos_context prefix demo' -a success -d 'Notification fixture'
complete -c notjikctl -n '__tjikos_context prefix demo' -a warning -d 'Notification fixture'
complete -c notjikctl -n '__tjikos_context prefix demo' -a error -d 'Notification fixture'
complete -c notjikctl -n '__tjikos_context prefix demo' -a timer-done -d 'Notification fixture'
complete -c notjikctl -n '__tjikos_context prefix demo' -a actions -d 'Notification fixture'
complete -c notjikctl -n '__tjikos_context prefix demo' -a actions-multiple -d 'Notification fixture'
complete -c notjikctl -n '__tjikos_context prefix demo' -a actions-wrapped -d 'Notification fixture'
complete -c notjikctl -n '__tjikos_context prefix demo' -a resident -d 'Notification fixture'
complete -c notjikctl -n '__tjikos_context prefix demo' -a urgency -d 'Notification fixture'
complete -c notjikctl -n '__tjikos_context prefix demo' -a multiline -d 'Notification fixture'
complete -c notjikctl -n '__tjikos_context prefix demo' -a long -d 'Notification fixture'
complete -c notjikctl -n '__tjikos_context prefix demo' -a unicode -d 'Notification fixture'
complete -c notjikctl -n '__tjikos_context prefix demo' -a markup -d 'Notification fixture'
complete -c notjikctl -n '__tjikos_context prefix demo' -a timeout -d 'Notification fixture'
complete -c notjikctl -n '__tjikos_context prefix demo' -a update -d 'Notification fixture'
complete -c notjikctl -n '__tjikos_context prefix demo' -a grouped -d 'Notification fixture'
complete -c notjikctl -n '__tjikos_context prefix demo' -a stack -d 'Notification fixture'
complete -c notjikctl -n '__tjikos_context prefix demo' -a telegram-text -d 'Notification fixture'
complete -c notjikctl -n '__tjikos_context prefix demo' -a telegram-photo -d 'Notification fixture'
complete -c notjikctl -n '__tjikos_context prefix demo' -a telegram-voice -d 'Notification fixture'
complete -c notjikctl -n '__tjikos_context prefix demo' -a telegram-sticker -d 'Notification fixture'
complete -c notjikctl -n '__tjikos_context prefix demo' -a telegram-animated-sticker -d 'Notification fixture'
complete -c notjikctl -n '__tjikos_context prefix demo' -a telegram-gif -d 'Notification fixture'
complete -c notjikctl -n '__tjikos_context prefix demo' -a telegram-animated-emoji -d 'Notification fixture'
complete -c notjikctl -n '__tjikos_context prefix demo' -a telegram-custom-emoji -d 'Notification fixture'
complete -c notjikctl -n '__tjikos_context prefix demo' -a telegram-grouped -d 'Notification fixture'
