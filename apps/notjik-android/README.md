# Notjik for Android

A native Kotlin/Compose notification drawer for Pi: global history, device/app/run
search, expandable result cards, read/dismiss synchronization, pull-to-refresh, Android
system notifications and cross-device card actions.

Status cards show their state, tone and source detail; results cards keep their
label/value rows and duration visible even when collapsed. Android's notification shade
uses native styled text with the same status, results and body in its expanded view. The
system controls that layout. Producers should send plain body text plus structured
`data`, rather than embedding Telegram HTML. Historical SnapRAID HTML bodies remain
readable in both Android views. Progress, countdown, task and key/value cards also
render their structured metadata in the drawer.

The drawer opens in **Inbox**. The search field’s filter icon slides out the Inbox,
Unread and History choices with their counts. Selecting a filter closes the choices; the
icon stays highlighted for Unread or History. Pull down on the notification list to
refresh, including an empty inbox. Background updates stay automatic. Notification sound
and interruption are controlled by Android’s system-wide Do Not Disturb and channel
settings; there is no separate app pause.

**Mark read** removes the unread indicator and the phone's system notification; the card
stays in Inbox. **Dismiss** removes the card from Inbox and Unread immediately, keeping
it in **History**. The gap closes while saving continues in the background; a failed
local save restores the card. Stale refreshes cannot undo a dismissal. These are
separate states. Cards follow freely after a horizontal drag starts. On release,
position and flick velocity project where the card will land: outside either edge clears
it; otherwise it springs back. Vertical or diagonal gestures initially remain list
scrolling; turning clearly sideways picks up the card during the same touch, without a
jump. Tap a card to reveal or hide its full body, run identifiers and actions, including
Copy code and the optional Mark read and Dismiss buttons. A thin centered bar at the
bottom matches the desktop's interaction indicator; action buttons stay hidden until
expanded. Scrolling a card's top edge beneath the fixed toolbar marks it read silently.
The list leaves enough space below the last card to cross that edge even with one card.
Expanding or dragging sideways does not mark it read. Swiping lifts and tilts the card;
expansion uses one height animation without a card-wide ripple. Search and the selected
filter stay in place after changes. Offline actions are saved atomically and shown as
waiting to sync; stale history responses cannot reverse a local read or dismissal.

Pi needs Google Play services, notification permission, background mobile data enabled
for Notjik, and Tailscale connectivity to Si. Open the app and enroll against
`https://si.tailb49776.ts.net/notjik`. Firebase receives only an opaque wakeup
reference. Notification text and actions are fetched over the private connection;
background phone use never takes priority from an active desktop. Opening or interacting
with the drawer updates Pi's activity time; periodic history refreshes do not. A newer
interaction on a desktop takes priority even while the phone drawer remains open.

Build with JDK 21, Android SDK platform/build-tools 35, and Gradle 8.13. Dotjik installs
`temurin-21-jdk` on Fi and Li through Tjikup's RPM package workflow using the signed
Adoptium repository. Fedora 45 no longer packages OpenJDK 21. Select the JDK explicitly
without changing the system Java default:

```sh
export JAVA_HOME=/usr/lib/jvm/temurin-21-jdk
./gradlew :app:testDebugUnitTest :app:assembleDebug
```

For a persistent signed sideload build, provide these environment variables from private
storage, then run `./gradlew :app:assembleRelease :app:bundleRelease`:

- `NOTJIK_ANDROID_KEYSTORE`
- `NOTJIK_ANDROID_STORE_PASSWORD`
- `NOTJIK_ANDROID_KEY_PASSWORD`
- `NOTJIK_ANDROID_KEY_ALIAS` (defaults to `notjik`)

Outputs are `app/build/outputs/apk/release/app-release.apk` and
`app/build/outputs/bundle/release/app-release.aab`. Keep the signing key outside Git and
retain it for updates and future Play signing enrollment. No Firebase configuration is
compiled into the application: Si supplies the public client identifiers during
authenticated enrollment. Application data and credentials use private Android storage,
with Android backup disabled.

`./gradlew :app:testDebugUnitTest` covers filters, search and presence. With the release
signing variables set and a device connected, `:app:connectedReleaseAndroidTest` checks
offline mutations and stale-history reconciliation in isolated test databases; it does
not modify the real notification cache.

FCM delivery is retried by WorkManager, including a 15-minute reconciliation job.
Android background limits and a disconnected Tailscale tunnel can delay content
fetching. Disabling the app's background mobile data can prevent private DNS and content
fetching even when Firebase delivers its wakeup. A new push starts its own expedited
reconciliation instead of waiting behind an older retry chain. The application keeps
saved history available offline and queues user mutations durably. Force-stopping the
app disables Android background delivery until it is opened again.

Authentication cards show **Copy code** as the first native notification action. It
briefly opens a foreground activity to copy on this phone, marks the clipboard
sensitive, and dismisses the local notification. It does not invoke a command on the
originating desktop. Unlock is required before copying from the lock screen.
