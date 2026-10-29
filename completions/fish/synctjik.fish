# Fish completion for synctjik.
source (status dirname)/__tjikos.fish
complete -c synctjik -f
complete -c synctjik -n '__tjikos_context exact' -a help -d 'Show command help'
complete -c synctjik -n '__tjikos_context exact' -s V -d 'Show version'
complete -c synctjik -s h -d 'Show help'
complete -c synctjik -n '__tjikos_context exact' -a capabilities -d Capabilities
complete -c synctjik -n '__tjikos_context exact' -a migrate-schedules -d 'Migrate schedules'
complete -c synctjik -n '__tjikos_context exact' -a cloudflare -d Cloudflare
complete -c synctjik -n '__tjikos_context exact' -a private-assets -d 'Private assets'
complete -c synctjik -n '__tjikos_context exact' -a protection-migration -d 'Protection migration'
complete -c synctjik -n '__tjikos_context exact' -a drive -d Drive
complete -c synctjik -n '__tjikos_context exact' -a app-backup -d 'App backup'
complete -c synctjik -n '__tjikos_context exact' -a app-pull -d 'App pull'
complete -c synctjik -n '__tjikos_context exact' -a app-restore -d 'App restore'
complete -c synctjik -n '__tjikos_context exact' -a snapraid -d Snapraid
complete -c synctjik -n '__tjikos_context exact' -a snapraid-notify -d 'Snapraid notify'
complete -c synctjik -n '__tjikos_context exact' -a check -d Check
complete -c synctjik -n '__tjikos_context exact' -a schedule -d Schedule
complete -c synctjik -n '__tjikos_context exact' -a ready -d Ready
complete -c synctjik -n '__tjikos_context exact' -a status -d Status
complete -c synctjik -n '__tjikos_context exact' -a devices -d Devices
complete -c synctjik -n '__tjikos_context exact' -a backup -d Backup
complete -c synctjik -n '__tjikos_context exact' -a prune -d Prune
complete -c synctjik -n '__tjikos_context exact' -a plan -d Plan
complete -c synctjik -n '__tjikos_context exact' -a sync -d Sync
complete -c synctjik -n '__tjikos_context exact' -a setup -d Setup
complete -c synctjik -n '__tjikos_context exact' -a apply -d Apply
complete -c synctjik -n '__tjikos_context exact' -a review -d Review
complete -c synctjik -n '__tjikos_context exact' -a restore -d Restore
complete -c synctjik -n '__tjikos_context exact' -a history -d History
complete -c synctjik -n '__tjikos_context exact' -a diff -d Diff
complete -c synctjik -n '__tjikos_context exact' -a enroll -d Enroll
complete -c synctjik -l config -d Config -r -F
complete -c synctjik -l home -d Home -r -F
complete -c synctjik -l machine -d Machine -r -f
complete -c synctjik -l state -d State -r -F
complete -c synctjik -l json -d Json
complete -c synctjik -l help -d Help
complete -c synctjik -l version -d Version
complete -c synctjik -n '__tjikos_context exact' -l demo -d Demo
complete -c synctjik -n '__tjikos_context prefix migrate-schedules' -l retire -d Retire
complete -c synctjik -n '__tjikos_context prefix schedule' -l print -d Print
complete -c synctjik -n '__tjikos_context prefix schedule' -l role -x -a 'desktop laptop server' -d Role
complete -c synctjik -n '__tjikos_context prefix status' -l devices -d Devices -r -f
complete -c synctjik -n '__tjikos_context prefix status' -l targets -d Targets -r -f
complete -c synctjik -n '__tjikos_context prefix backup' -l force -d Force
complete -c synctjik -n '__tjikos_context prefix prune' -l apply -d Apply
complete -c synctjik -n '__tjikos_context prefix plan' -l from -d From -r -f
complete -c synctjik -n '__tjikos_context prefix sync' -l from -d From -r -f
complete -c synctjik -n '__tjikos_context prefix setup' -l from -d From -r -f
complete -c synctjik -n '__tjikos_context prefix restore' -l from -d From -r -f
complete -c synctjik -n '__tjikos_context prefix history' -l from -d From -r -f
complete -c synctjik -n '__tjikos_context prefix enroll' -l from -d From -r -f
complete -c synctjik -n '__tjikos_context prefix plan' -l snapshot -d Snapshot -r -f
complete -c synctjik -n '__tjikos_context prefix sync' -l snapshot -d Snapshot -r -f
complete -c synctjik -n '__tjikos_context prefix setup' -l snapshot -d Snapshot -r -f
complete -c synctjik -n '__tjikos_context prefix restore' -l snapshot -d Snapshot -r -f
complete -c synctjik -n '__tjikos_context prefix apply' -l resolutions -d Resolutions -r -F
complete -c synctjik -n '__tjikos_context exact apply' -F
complete -c synctjik -n '__tjikos_context exact review' -F
complete -c synctjik -n '__tjikos_context prefix review' -l file -d File -r -f
complete -c synctjik -n '__tjikos_context prefix restore' -l to -d To -r -F
complete -c synctjik -n '__tjikos_context prefix diff' -l left -d Left -r -f
complete -c synctjik -n '__tjikos_context prefix diff' -l right -d Right -r -f
complete -c synctjik -n '__tjikos_context prefix diff' -l file -d File -r -f
complete -c synctjik -n '__tjikos_context prefix enroll' -l role -x -a 'desktop laptop server' -d Role
complete -c synctjik -n '__tjikos_context prefix enroll' -l collections -d Collections -r -f
complete -c synctjik -n '__tjikos_context prefix enroll' -l replace -d Replace
complete -c synctjik -n '__tjikos_context exact cloudflare' -a backup -d Backup
complete -c synctjik -n '__tjikos_context exact cloudflare' -a check -d Check
complete -c synctjik -n '__tjikos_context prefix cloudflare' -l policy -d Policy -r -F
complete -c synctjik -n '__tjikos_context exact drive' -a backup -d Backup
complete -c synctjik -n '__tjikos_context exact drive' -a check -d Check
complete -c synctjik -n '__tjikos_context exact drive' -a snapshots -d Snapshots
complete -c synctjik -n '__tjikos_context exact drive' -a restore -d Restore
complete -c synctjik -n '__tjikos_context prefix drive' -l policy -d Policy -r -F
complete -c synctjik -n '__tjikos_context exact app-backup' -a backup -d Backup
complete -c synctjik -n '__tjikos_context exact app-backup' -a check -d Check
complete -c synctjik -n '__tjikos_context prefix app-backup' -l policy -d Policy -r -F
complete -c synctjik -n '__tjikos_context exact app-pull' -a copy -d Copy
complete -c synctjik -n '__tjikos_context exact app-pull' -a health -d Health
complete -c synctjik -n '__tjikos_context exact app-pull' -a check -d Check
complete -c synctjik -n '__tjikos_context prefix app-pull' -l policy -d Policy -r -F
complete -c synctjik -n '__tjikos_context exact app-restore' -a stage -d Stage
complete -c synctjik -n '__tjikos_context exact app-restore' -a install -d Install
complete -c synctjik -n '__tjikos_context exact app-restore' -a import-mariadb -d 'Import mariadb'
complete -c synctjik -n '__tjikos_context exact app-restore' -a verify -d Verify
complete -c synctjik -n '__tjikos_context prefix app-restore' -l policy -d Policy -r -F
complete -c synctjik -n '__tjikos_context exact snapraid' -a check -d Check
complete -c synctjik -n '__tjikos_context exact snapraid' -a guard -d Guard
complete -c synctjik -n '__tjikos_context exact snapraid' -a status -d Status
complete -c synctjik -n '__tjikos_context exact snapraid' -a preflight -d Preflight
complete -c synctjik -n '__tjikos_context exact snapraid' -a sync -d Sync
complete -c synctjik -n '__tjikos_context exact snapraid' -a scrub -d Scrub
complete -c synctjik -n '__tjikos_context exact snapraid' -a resume -d Resume
complete -c synctjik -n '__tjikos_context prefix snapraid' -l policy -d Policy -r -F
complete -c synctjik -n '__tjikos_context exact snapraid-notify' -a check -d Check
complete -c synctjik -n '__tjikos_context exact snapraid-notify' -a failure -d Failure
complete -c synctjik -n '__tjikos_context exact snapraid-notify' -a recovery -d Recovery
complete -c synctjik -n '__tjikos_context exact snapraid-notify' -a health -d Health
complete -c synctjik -n '__tjikos_context prefix snapraid-notify' -l policy -d Policy -r -F
complete -c synctjik -n '__tjikos_context prefix cloudflare' -l dry-run -d 'Dry run'
complete -c synctjik -n '__tjikos_context prefix drive' -l snapshot -d Snapshot -r -f
complete -c synctjik -n '__tjikos_context prefix drive' -l to -d To -r -F
complete -c synctjik -n '__tjikos_context prefix drive' -l tag -d Tag -r -f
complete -c synctjik -n '__tjikos_context prefix app-pull' -l health-check -d 'Health check'
complete -c synctjik -n '__tjikos_context prefix app-restore' -l stage -d Stage -r -F
complete -c synctjik -n '__tjikos_context prefix app-restore' -l repository -d Repository -r -f
complete -c synctjik -n '__tjikos_context prefix app-restore' -l password-file -d 'Password file' -r -F
complete -c synctjik -n '__tjikos_context prefix app-restore' -l snapshot -d Snapshot -r -f
complete -c synctjik -n '__tjikos_context prefix app-restore' -l include -d Include -r -F
complete -c synctjik -n '__tjikos_context prefix protection-migration' -l policy -d Policy -r -F
complete -c synctjik -n '__tjikos_context exact private-assets' -a sync -d Sync
complete -c synctjik -n '__tjikos_context exact private-assets' -a init -d Init
complete -c synctjik -n '__tjikos_context exact private-assets' -a backup -d Backup
complete -c synctjik -n '__tjikos_context exact private-assets' -a snapshots -d Snapshots
complete -c synctjik -n '__tjikos_context exact private-assets' -a restore -d Restore
complete -c synctjik -n '__tjikos_context exact private-assets' -a status -d Status
complete -c synctjik -n '__tjikos_context exact private-assets' -a ready -d Ready
complete -c synctjik -n '__tjikos_context exact private-assets' -a crypt-password -d 'Crypt password'
complete -c synctjik -n '__tjikos_context exact private-assets' -a mount -d Mount
complete -c synctjik -n '__tjikos_context prefix private-assets sync' -l progress -d Progress
complete -c synctjik -n '__tjikos_context prefix private-assets sync' -l max-delete-percent -d 'Max delete percent' -r -f
complete -c synctjik -n '__tjikos_context prefix private-assets backup' -l scheduled -d Scheduled
complete -c synctjik -n '__tjikos_context prefix private-assets snapshots' -l from -x -a 'fi li' -d From
complete -c synctjik -n '__tjikos_context prefix private-assets restore' -l from -x -a 'fi li' -d From
complete -c synctjik -n '__tjikos_context prefix private-assets restore' -l snapshot -d Snapshot -r -f
complete -c synctjik -n '__tjikos_context prefix private-assets restore' -l to -d To -r -F
complete -c synctjik -n '__tjikos_context exact private-assets mount' -a crypt -d Crypt
complete -c synctjik -n '__tjikos_context exact private-assets mount' -a giprivate -d Giprivate
complete -c synctjik -n '__tjikos_context exact private-assets mount' -a gipublic -d Gipublic
complete -c synctjik -n '__tjikos_context exact private-assets mount' -a siprivate -d Siprivate
complete -c synctjik -n '__tjikos_context exact private-assets mount' -a sipublic -d Sipublic
