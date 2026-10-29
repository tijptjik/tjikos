# Shared completion context. Never invokes an application, service, or network.
function __tjikos_context
    set -l mode $argv[1]
    set -e argv[1]
    set -l words
    set -l skip 0
    set -l tokens (commandline -opc)
    for token in $tokens[2..-1]
        if test $skip -eq 1
            set skip 0
            continue
        end
        if test "$token" = --
            return 1
        end
        if test "$token" = --replace; and test "$tokens[1]" = notjikctl
            set skip 1
        else if contains -- "$token" --config --home --machine --state --repo --policy --jobs --manifest --state-dir --root --context-file --group --receipt --output --role --subscribe --from --snapshot --to --resolutions --file --left --right --collections --transport --run-dir --unit --exclude-unit --channel --input --preset --theme --app --type --urgency --timeout --icon --image --id --data --actions --limit --event --since --interval --dunst --stage --repository --password-file --include --tag --max-delete-percent --devices --targets
            set skip 1
        else if not string match -q -- '-*' "$token"
            set -a words "$token"
        end
    end
    if test "$mode" = exact
        test $skip -eq 0; or return 1
        test (count $words) -eq (count $argv); or return 1
    else
        test (count $words) -ge (count $argv); or return 1
    end
    for i in (seq (count $argv))
        test "$words[$i]" = "$argv[$i]"; or return 1
    end
end
