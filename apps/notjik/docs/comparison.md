# Side-by-side native notification comparison

```sh
notjikctl compare start
notjikctl compare case basic
```

The optional reference binary is read from `--dunst /path/to/dunst`, PATH, or
`~/.cache/notjik/comparison-reference/usr/bin/dunst`. It does not need to be
installed as a system package or enabled as a service. On this workstation a
Fedora reference build was extracted into that cache solely for this comparison.

The harness runs two private D-Bus sessions with desktop-service activation
disabled. Dunst appears bottom-centre on the primary display; the comparison
notjik appears **400 physical pixels to its right**. Both select the monitor by
connector name; output indexes are not interchangeable between the two servers. Both use 360-pixel cards to
leave a gap. Test grouping is disabled. These settings and test history live under
`$XDG_RUNTIME_DIR/notjik-compare-$UID`; production configuration is unchanged.

Ordinary `notify-send` still targets the production server. To send exactly the
same arguments to both test servers, prefix them with `notjikctl compare send --`:

```sh
notjikctl compare send -- -u normal -t 8000 'Connected' 'Wi-Fi is ready'
notjikctl compare send -- -u critical -t 0 'Disk full' 'Requires attention'
notjikctl compare send -- -h int:value:45 'Uploading' '45 percent'
```

Run each line as a separate command. Joining the end of one body directly to
`notjikctl` produces `Invalid number of options`; it does not mean the servers
have stopped. Check the normal service with `notjikctl status` and resume paused
delivery with `notjikctl resume`.

The output reports each server's notification ID and selected action. IDs can
differ between servers. Use the built-in replacement case to map each ID correctly;
passing a single `--replace-id` yourself sends that number literally to both buses.

## Ready-made cases

Run one at a time, clearing between cases if desired:

```sh
notjikctl compare cases
notjikctl compare clear
notjikctl compare case icon
```

| Case | What to compare |
| --- | --- |
| `basic`, `low`, `normal`, `critical` | Body, title and urgency; critical remains until dismissed |
| `timeout`, `sticky` | 2.5-second expiration versus until-dismissed |
| `icon`, `image` | Icon-theme artwork versus synthetic bitmap/image-data |
| `progress` | Native integer `value` hint |
| `replace` | 10% changes to 75% in the same notification, with separate backend IDs |
| `stack` | Four simultaneous full-size cards, spacing and dismissal animation |
| `multiline`, `long`, `unicode` | Wrapping, truncation, line breaks and multilingual text |
| `actions` | Action signals; click Open in notjik and middle-click the Dunst card for its default action |
| `resident` | One action; the card should remain after invoking it |
| `transient` | Exclusion from persistent history |
| `markup` | Explicit compatibility gap: notjik currently shows literal body markup |

Action cases wait for a selection/close from both servers, for up to 30 seconds
by default. Standard actions report names to the sender; these fixtures do not
execute application commands. Dunst uses a default action/context-menu interaction;
notjik renders explicit buttons. A normal background click dismisses either card.

```sh
notjikctl compare status
notjikctl compare clear   # dismiss only comparison notifications
notjikctl compare stop    # terminate only the two test servers and private buses
```

## Compatibility boundaries

Both advertise body text, static icons and actions. Dunst also advertises body
markup, hyperlinks and vendor-specific synchronous/stack-tag extensions; notjik
currently does not. Notjik advertises persistence. These are capability differences,
not proof of equivalent presentation. Ordering, wrapping, action presentation and
timeout policy also need visual comparison. The `markup` case deliberately exposes
the plain-text boundary rather than silently claiming full compatibility.

Automated checks exercised paired native sends, image-data, progress, replacement
and the `ActionInvoked` response. The production notification bus owner remained
separate throughout the comparison.

## Rich demo layouts beside the reference

`notjikctl demo` automatically starts this comparison and shows the overview.
Use `notjikctl demo actions-wrapped`, `demo update`, or `demo all` to review the
named variations beside Dunst. Native fields are mirrored; status and timer
components have a labelled textual baseline on Dunst. Native Dunst actions return
immediately when sent for demos, so previewing buttons never stalls the gallery.
`demo clear` removes only paired demo IDs; `compare clear` clears every fixture
in both private servers. Use `demo --solo` for the normal notjik placement.

Paired demos resolve the artwork from the actual Notjik notice, including its
random robot fallback, and pass that same image to Dunst. Each renderer retains
its own layout and image sizing. This also applies to the robot finale.
