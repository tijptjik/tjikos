# Detached workspace lifecycle

Dragging a card uses a temporary layer surface while the pointer owns the drag. On
release, a worker prepares an exact-title floating-window rule through `tjik-desktop`.
Only after preparation succeeds does GTK map the ordinary, undecorated toplevel. The
rule assigns the original workspace and geometry silently, without initial focus.

The existing card and reply draft move into that window. A frozen rendered face stays on
the old surface until the native window paints, with a 250 ms cleanup fallback.
Temporary placement rules are released after mapping, including cancelled handoffs.
Subsequent workspace transitions and window movement belong to the compositor. Native
windows survive output removal; the compositor relocates them normally.

Lock/conceal hides native card content and disables input without remapping the window.
Explicit close still dismisses the corresponding notification. Popup stacks retain their
layer-shell behavior. Unsupported native placement retains the detached layer surface
and reports the adapter error; it does not claim native-window support.

Validation includes the synthetic `native_detached_workspace_window` fixture, which
assigns workspace 7 before mapping and checks floating placement, no initial focus,
widget/draft identity, and scoped dismissal. Live workspace transition appearance and
physical multi-monitor dragging remain acceptance checks.
