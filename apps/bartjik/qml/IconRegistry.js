// ProIcons b8718fbc0a8654f6065cff8b3166ee3b99cd89ca; semantic names preserve provider independence.
var proicons = {
    "wifi": "wi-fi",
    "headphones": "headphones",
    "volume_off": "volume-mute",
    "volume_up": "volume",
    "notifications": "bell",
    "notifications_off": "bell-off",
    "coffee": "coffee-hot",
    "nightlight": "moon",
    "light_mode": "brightness",
    "desktop_windows": "computer",
    "sports_esports": "game",
    "shield": "shield",
    "shield_lock": "shield-keyhole",
    "vpn_lock": "shield-keyhole",
    "play_arrow": "play",
    "pause": "pause",
    "stop": "record-stop"
};
function resolve(symbol, set) {
    if(symbol==="hotel" && set!=="material")return "icons/bed.svg";
    return set === "material" || !proicons[symbol] ? "" : "icons/proicons/" + proicons[symbol] + ".svg";
}
