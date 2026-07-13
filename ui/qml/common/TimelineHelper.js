.pragma library

function prop(obj, name, fallback) {
    if (obj == null)
        return fallback;
    var v = obj[name];
    return (v !== undefined && v !== null) ? v : fallback;
}

function invoke(obj /*, ...names */) {
    var cur = obj;
    for (var i = 1; i < arguments.length; i++) {
        if (cur == null)
            return;
        cur = cur[arguments[i]];
    }
    if (cur && typeof cur === "function")
        cur();
}

function invokeWith(obj, names, fn) {
    var cur = obj;
    for (var i = 0; i < names.length; i++) {
        if (cur == null)
            return;
        cur = cur[names[i]];
    }
    if (cur)
        fn(cur);
}

function scaleToPercent(scale) {
    return scale <= 1 ? scale * 100 : 100 + ((scale - 1) * 300 / 9);
}

function percentToScale(pct) {
    return pct <= 100 ? pct / 100 : 1 + ((pct - 100) * 9 / 300);
}
