case "reference-artwork": {
            const notice = find(request.id);
            if (!notice) throw Error("Notification not found");
            const fallback = Qt.resolvedUrl(Logic.robotArtwork(notice)).toString();
            const value = notice.image || notice.icon || fallback;
            const artwork = value.startsWith("/") || value.startsWith("file:") || value.startsWith("image:") ? value
                : value.startsWith("network-wireless") ? Qt.resolvedUrl("icons/wifi-cover.png").toString()
                : Quickshell.iconPath(value, true) || fallback;
            return {ok:true, image:artwork};
        }
        case "status": break;