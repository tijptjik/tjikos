.pragma library

function escaped(value) {
    return String(value).replace(/&/g, "&amp;").replace(/</g, "&lt;")
        .replace(/>/g, "&gt;").replace(/\"/g, "&quot;");
}

function text(value) {
    // Match Tjikup's terminal completion banner, including spaces in the cycle.
    const colors = ["#ff5f5f", "#ffaf00", "#5fff87", "#5fd7ff", "#5f87ff", "#d787ff"];
    return Array.from(String(value)).map((character, index) =>
        character === " " ? " " : "<font color=\"" + colors[index % colors.length]
            + "\">" + escaped(character) + "</font>").join("");
}
