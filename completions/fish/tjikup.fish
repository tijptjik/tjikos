# Fish completion for tjikup.
source (status dirname)/__tjikos.fish
complete -c tjikup -f
complete -c tjikup -n '__tjikos_context exact' -l upgrade -d 'Upgrade applications through declared recipes'
complete -c tjikup -n '__tjikos_context exact' -l skip-repo-sync -d 'Use local policy checkouts'
complete -c tjikup -n '__tjikos_context exact' -l skip-conflicts -d 'Skip conflicting optional work'
complete -c tjikup -n '__tjikos_context exact' -a config -d 'Manage home and system configuration'
complete -c tjikup -n '__tjikos_context exact' -a files -d 'Deploy native file manifests'
complete -c tjikup -n '__tjikos_context exact' -a git -d 'Synchronize declared repositories'
complete -c tjikup -n '__tjikos_context exact' -a ssh -d 'Connect using pinned fleet identity'
complete -c tjikup -n '__tjikos_context exact' -a system -d 'Run native host maintenance'
complete -c tjikup -n '__tjikos_context exact' -a workflow -d 'Run native policy helpers'
complete -c tjikup -n '__tjikos_context exact' -a sync -d 'Run Synctjik archive commands'
complete -c tjikup -n '__tjikos_context exact' -a new -d 'Enroll a device'
complete -c tjikup -n '__tjikos_context exact' -a notify -d 'Deliver notification metadata'
complete -c tjikup -n '__tjikos_context exact' -a capabilities -d 'Show supported features'
complete -c tjikup -n '__tjikos_context exact' -l help -d Help
complete -c tjikup -n '__tjikos_context exact' -l version -d Version
complete -c tjikup -n '__tjikos_context exact' -l check -d Check
complete -c tjikup -n '__tjikos_context exact' -l plan -d Plan
complete -c tjikup -n '__tjikos_context exact' -l dry-run -d 'Dry run'
complete -c tjikup -n '__tjikos_context exact' -l refresh-secrets -d 'Refresh secrets'
complete -c tjikup -n '__tjikos_context exact' -l alphabetical -d Alphabetical
complete -c tjikup -n '__tjikos_context exact' -l show-skips -d 'Show skips'
complete -c tjikup -n '__tjikos_context exact' -l show-skips-and-other-devices -d 'Show skips and other devices'
complete -c tjikup -n '__tjikos_context exact' -l apply-dotfiles -d 'Apply dotfiles'
complete -c tjikup -n '__tjikos_context exact' -l apply-sysfiles -d 'Apply sysfiles'
complete -c tjikup -n '__tjikos_context exact' -l apply-package-updates -d 'Apply package updates'
complete -c tjikup -n '__tjikos_context exact' -l skip-package-updates -d 'Skip package updates'
complete -c tjikup -n '__tjikos_context exact' -l config -d Config -r -F
complete -c tjikup -n '__tjikos_context exact' -l jobs -d Jobs -r -f
complete -c tjikup -n '__tjikos_context exact' -l demo -x -a 'changed unchanged attention' -d Demo
complete -c tjikup -n '__tjikos_context exact config' -a home -d Home
complete -c tjikup -n '__tjikos_context exact config' -a system -d System
complete -c tjikup -n '__tjikos_context exact config home' -a check -d Check
complete -c tjikup -n '__tjikos_context exact config home' -a plan -d Plan
complete -c tjikup -n '__tjikos_context exact config home' -a diff -d Diff
complete -c tjikup -n '__tjikos_context exact config home' -a apply -d Apply
complete -c tjikup -n '__tjikos_context exact config home' -a source-path -d 'Source path'
complete -c tjikup -n '__tjikos_context exact config home' -a edit -d Edit
complete -c tjikup -n '__tjikos_context exact config home edit' -F
complete -c tjikup -n '__tjikos_context exact config home source-path' -F
complete -c tjikup -n '__tjikos_context exact config system' -a check -d Check
complete -c tjikup -n '__tjikos_context exact config system' -a plan -d Plan
complete -c tjikup -n '__tjikos_context exact config system' -a diff -d Diff
complete -c tjikup -n '__tjikos_context exact config system' -a apply -d Apply
complete -c tjikup -n '__tjikos_context exact config system' -a source-path -d 'Source path'
complete -c tjikup -n '__tjikos_context exact config system' -a edit -d Edit
complete -c tjikup -n '__tjikos_context exact config system edit' -F
complete -c tjikup -n '__tjikos_context exact config system source-path' -F
complete -c tjikup -n '__tjikos_context exact files' -a check -d Check
complete -c tjikup -n '__tjikos_context exact files' -a plan -d Plan
complete -c tjikup -n '__tjikos_context exact files' -a apply -d Apply
complete -c tjikup -n '__tjikos_context exact files' -a restore -d Restore
complete -c tjikup -n '__tjikos_context prefix files' -l manifest -d Manifest -r -F
complete -c tjikup -n '__tjikos_context prefix files' -l state-dir -d 'State dir' -r -F
complete -c tjikup -n '__tjikos_context prefix files' -l root -d Root -r -F
complete -c tjikup -n '__tjikos_context prefix files' -l context-file -d 'Context file' -r -F
complete -c tjikup -n '__tjikos_context prefix files' -l group -d Group -r -f
complete -c tjikup -n '__tjikos_context prefix files' -l receipt -d Receipt -r -F
complete -c tjikup -n '__tjikos_context prefix files' -l json -d Json
complete -c tjikup -n '__tjikos_context prefix files' -l adopt-identical -d 'Adopt identical'
complete -c tjikup -n '__tjikos_context prefix files' -l refresh-secrets -d 'Refresh secrets'
complete -c tjikup -n '__tjikos_context exact git' -a push -d Push
complete -c tjikup -n '__tjikos_context exact git' -a sync -d Sync
complete -c tjikup -n '__tjikos_context prefix git' -l dry-run -d 'Dry run'
complete -c tjikup -n '__tjikos_context prefix git' -l verbose -d Verbose
complete -c tjikup -n '__tjikos_context prefix git' -s v -d 'Verbose output'
complete -c tjikup -n '__tjikos_context prefix ssh' -l transport -x -a 'auto lan tailnet' -d Transport
complete -c tjikup -n '__tjikos_context exact ssh' -a fi -d Fi
complete -c tjikup -n '__tjikos_context exact ssh' -a li -d Li
complete -c tjikup -n '__tjikos_context exact system' -a nss -d Nss
complete -c tjikup -n '__tjikos_context exact system' -a storage -d Storage
complete -c tjikup -n '__tjikos_context exact system' -a provision -d Provision
complete -c tjikup -n '__tjikos_context exact system' -a permissions -d Permissions
complete -c tjikup -n '__tjikos_context exact system' -a media-guard -d 'Media guard'
complete -c tjikup -n '__tjikos_context exact system' -a transmission-cleanup -d 'Transmission cleanup'
complete -c tjikup -n '__tjikos_context exact system' -a update-apps -d 'Update apps'
complete -c tjikup -n '__tjikos_context prefix system' -l policy -d Policy -r -F
complete -c tjikup -n '__tjikos_context prefix system nss' -l apply -d Apply
complete -c tjikup -n '__tjikos_context prefix system permissions' -l apply -d Apply
complete -c tjikup -n '__tjikos_context prefix system provision' -l apply -d Apply
complete -c tjikup -n '__tjikos_context prefix system media-guard' -l dry-run -d 'Dry run'
complete -c tjikup -n '__tjikos_context prefix system transmission-cleanup' -l dry-run -d 'Dry run'
complete -c tjikup -n '__tjikos_context prefix system transmission-cleanup' -l apply -d Apply
complete -c tjikup -n '__tjikos_context prefix system storage' -l check -d Check
complete -c tjikup -n '__tjikos_context prefix system storage' -l check-docker -d 'Check docker'
complete -c tjikup -n '__tjikos_context prefix system storage' -l print-fstab -d 'Print fstab'
complete -c tjikup -n '__tjikos_context prefix system storage' -l write-fstab -d 'Write fstab'
complete -c tjikup -n '__tjikos_context exact system provision' -a accounts -d Accounts
complete -c tjikup -n '__tjikos_context exact system provision' -a directories -d Directories
complete -c tjikup -n '__tjikos_context exact system provision' -a mergerfs -d Mergerfs
complete -c tjikup -n '__tjikos_context exact system provision' -a beets -d Beets
complete -c tjikup -n '__tjikos_context exact new' -a device -d Device
complete -c tjikup -n '__tjikos_context prefix new device' -l config -d Config -r -F
complete -c tjikup -n '__tjikos_context prefix new device' -l output -d Output -r -F
complete -c tjikup -n '__tjikos_context prefix new device' -l subscribe -d Subscribe -r -f
complete -c tjikup -n '__tjikos_context prefix new device' -l replace -d Replace
complete -c tjikup -n '__tjikos_context prefix new device' -l role -x -a 'desktop laptop server' -d Role
complete -c tjikup -n '__tjikos_context prefix notify' -l stdin -d Stdin
complete -c tjikup -n '__tjikos_context exact workflow' -a context -d Context
complete -c tjikup -n '__tjikos_context exact workflow' -a role -d Role
complete -c tjikup -n '__tjikos_context exact workflow' -a configure -d Configure
complete -c tjikup -n '__tjikos_context exact workflow' -a files -d Files
complete -c tjikup -n '__tjikos_context exact workflow' -a bootstrap-archives -d 'Bootstrap archives'
complete -c tjikup -n '__tjikos_context exact workflow' -a package -d Package
complete -c tjikup -n '__tjikos_context exact workflow' -a repositories -d Repositories
complete -c tjikup -n '__tjikos_context exact workflow' -a bitwarden -d Bitwarden
complete -c tjikup -n '__tjikos_context exact workflow' -a install -d Install
complete -c tjikup -n '__tjikos_context exact workflow' -a desktop -d Desktop
complete -c tjikup -n '__tjikos_context exact workflow' -a timing -d Timing
complete -c tjikup -n '__tjikos_context exact workflow' -a assets -d Assets
complete -c tjikup -n '__tjikos_context exact workflow' -a services -d Services
complete -c tjikup -n '__tjikos_context exact workflow' -a activate -d Activate
complete -c tjikup -n '__tjikos_context exact workflow' -a verify -d Verify
complete -c tjikup -n '__tjikos_context exact workflow' -a cache-install -d 'Cache install'
complete -c tjikup -n '__tjikos_context exact workflow' -a capture -d Capture
complete -c tjikup -n '__tjikos_context exact workflow' -a fedora-release -d 'Fedora release'
complete -c tjikup -n '__tjikos_context prefix workflow' -l repo -d Repo -r -F
complete -c tjikup -n '__tjikos_context exact workflow files' -a home -d Home
complete -c tjikup -n '__tjikos_context exact workflow files' -a home-files -d 'Home files'
complete -c tjikup -n '__tjikos_context exact workflow files' -a credentials -d Credentials
complete -c tjikup -n '__tjikos_context exact workflow files' -a system -d System
complete -c tjikup -n '__tjikos_context exact workflow files home' -a check -d Check
complete -c tjikup -n '__tjikos_context exact workflow files home' -a plan -d Plan
complete -c tjikup -n '__tjikos_context exact workflow files home' -a diff -d Diff
complete -c tjikup -n '__tjikos_context exact workflow files home' -a apply -d Apply
complete -c tjikup -n '__tjikos_context exact workflow files home-files' -a check -d Check
complete -c tjikup -n '__tjikos_context exact workflow files home-files' -a plan -d Plan
complete -c tjikup -n '__tjikos_context exact workflow files home-files' -a diff -d Diff
complete -c tjikup -n '__tjikos_context exact workflow files home-files' -a apply -d Apply
complete -c tjikup -n '__tjikos_context exact workflow files credentials' -a check -d Check
complete -c tjikup -n '__tjikos_context exact workflow files credentials' -a plan -d Plan
complete -c tjikup -n '__tjikos_context exact workflow files credentials' -a diff -d Diff
complete -c tjikup -n '__tjikos_context exact workflow files credentials' -a apply -d Apply
complete -c tjikup -n '__tjikos_context exact workflow files system' -a check -d Check
complete -c tjikup -n '__tjikos_context exact workflow files system' -a plan -d Plan
complete -c tjikup -n '__tjikos_context exact workflow files system' -a diff -d Diff
complete -c tjikup -n '__tjikos_context exact workflow files system' -a apply -d Apply
complete -c tjikup -n '__tjikos_context prefix workflow files' -l group -d Group -r -f
complete -c tjikup -n '__tjikos_context exact workflow package' -a check -d Check
complete -c tjikup -n '__tjikos_context exact workflow package' -a skip-update -d 'Skip update'
complete -c tjikup -n '__tjikos_context exact workflow package' -a run -d Run
complete -c tjikup -n '__tjikos_context exact workflow repositories' -a check -d Check
complete -c tjikup -n '__tjikos_context exact workflow repositories' -a apply -d Apply
complete -c tjikup -n '__tjikos_context prefix workflow bitwarden' -l upgrade -d Upgrade
complete -c tjikup -n '__tjikos_context prefix workflow install' -l metadata -d Metadata
complete -c tjikup -n '__tjikos_context exact workflow desktop' -a icons -d Icons
complete -c tjikup -n '__tjikos_context exact workflow desktop' -a cleanup -d Cleanup
complete -c tjikup -n '__tjikos_context exact workflow desktop' -a zed -d Zed
complete -c tjikup -n '__tjikos_context prefix workflow desktop cleanup' -l check -d Check
complete -c tjikup -n '__tjikos_context prefix workflow desktop cleanup' -l apply -d Apply
complete -c tjikup -n '__tjikos_context prefix workflow desktop zed' -l channel -x -a 'stable preview nightly' -d Channel
complete -c tjikup -n '__tjikos_context exact workflow timing' -a take -d Take
complete -c tjikup -n '__tjikos_context exact workflow timing' -a accept -d Accept
complete -c tjikup -n '__tjikos_context exact workflow timing take' -F
complete -c tjikup -n '__tjikos_context exact workflow timing accept' -F
complete -c tjikup -n '__tjikos_context exact workflow assets' -a fonts -d Fonts
complete -c tjikup -n '__tjikos_context exact workflow assets' -a icons -d Icons
complete -c tjikup -n '__tjikos_context prefix workflow assets' -l run-dir -d 'Run dir' -r -F
complete -c tjikup -n '__tjikos_context prefix workflow services' -l check -d Check
complete -c tjikup -n '__tjikos_context prefix workflow services' -l unit -d Unit -r -f
complete -c tjikup -n '__tjikos_context prefix workflow services' -l exclude-unit -d 'Exclude unit' -r -f
complete -c tjikup -n '__tjikos_context exact workflow activate' -a bartjik -d Bartjik
complete -c tjikup -n '__tjikos_context exact workflow activate' -a notjik -d Notjik
complete -c tjikup -n '__tjikos_context prefix workflow cache-install' -l input -d Input -r -F
complete -c tjikup -n '__tjikos_context prefix workflow cache-install' -l output -d Output -r -F
complete -c tjikup -n '__tjikos_context prefix workflow capture' -l dry-run -d 'Dry run'
complete -c tjikup -n '__tjikos_context prefix sync' -a '(complete -C (string join " " -- synctjik (string escape -- (commandline -opc)[3..-1]) (string escape -- (commandline -ct))))'

complete -c tjikup -n '__tjikos_context exact' -a scheduled -d 'Weekly workflow with pinned Telegram status'
complete -c tjikup -n '__tjikos_context exact scheduled' -l notify-only -d 'Retry the saved Telegram status'
complete -c tjikup -n '__tjikos_context exact scheduled' -l notify-test -d 'Verify Telegram delivery without maintenance'
