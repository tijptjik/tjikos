# Detached workspace lifecycle

Detached cards retain their card widgets and mapped layer surface across workspace
changes. While away, the scene is hidden, keyboard ownership is released and the input
region is empty. Returning restores the scene and its existing stack slots; it does not
remap the layer and trigger the compositor entrance animation.

Explicit dismissal, locking and normal notification lifecycle remain separate from
workspace visibility. This retains layer-shell positioning; it does not turn cards into
compositor-managed toplevel windows or add workspace-slide animations.
