# Notjik for Android

A native Kotlin/Compose notification drawer for Pi: global history, device/app/run
search, expandable result cards, read/dismiss synchronization, per-device pause,
Android system notifications and cross-device card actions.

The drawer opens in **Inbox**. **Mark read** removes the unread indicator and the
phone's system notification; the card stays in Inbox. **Dismiss** removes the card
from Inbox and Unread, keeping it in **History**. These are separate states.
Controls are visible on each active card; details reveal run identifiers, full
results, and source-device actions. Search and the selected filter stay in place
after changes. Offline actions are saved atomically and shown as waiting to sync;
stale history responses cannot reverse a local read or dismissal.

Pi needs Google Play services, notification permission, background mobile data
enabled for Notjik, and Tailscale connectivity
to Si. Open the app and enroll against `https://si.tailb49776.ts.net/notjik`.
Firebase receives only an opaque wakeup reference. Notification text and actions
are fetched over the private connection; background phone use never takes priority
from an active desktop. Opening or interacting with the drawer updates Pi's
activity time; periodic history refreshes do not. A newer interaction on a desktop
takes priority even while the phone drawer remains open.

Build with JDK 21, Android SDK platform/build-tools 35, and Gradle 8.13:

```sh
./gradlew :app:assembleDebug
```

For a persistent signed sideload build, provide these environment variables from
private storage, then run `./gradlew :app:assembleRelease :app:bundleRelease`:

- `NOTJIK_ANDROID_KEYSTORE`
- `NOTJIK_ANDROID_STORE_PASSWORD`
- `NOTJIK_ANDROID_KEY_PASSWORD`
- `NOTJIK_ANDROID_KEY_ALIAS` (defaults to `notjik`)

Outputs are `app/build/outputs/apk/release/app-release.apk` and
`app/build/outputs/bundle/release/app-release.aab`. Keep the signing key outside
Git and retain it for updates and future Play signing enrollment. No Firebase
configuration is compiled into the application: Si supplies the public client
identifiers during authenticated enrollment. Application data and credentials use
private Android storage, with Android backup disabled.

`./gradlew :app:testDebugUnitTest` covers filters, search and presence. With the
release signing variables set and a device connected, `:app:connectedReleaseAndroidTest`
checks offline mutations and stale-history reconciliation in isolated test databases;
it does not modify the real notification cache.

FCM delivery is retried by WorkManager, including a 15-minute reconciliation job.
Android background limits and a disconnected Tailscale tunnel can delay content
fetching. Disabling the app's background mobile data can prevent private DNS and
content fetching even when Firebase delivers its wakeup. A new push starts its
own expedited reconciliation instead of waiting behind an older retry chain.
The application keeps saved history available offline and queues user
mutations durably. Force-stopping the app disables Android background delivery
until it is opened again.
