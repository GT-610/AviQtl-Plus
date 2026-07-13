.pragma library

function scaleToPercent(scale) {
    return scale <= 1 ? scale * 100 : 100 + ((scale - 1) * 300 / 9);
}

function percentToScale(pct) {
    return pct <= 100 ? pct / 100 : 1 + ((pct - 100) * 9 / 300);
}
