.pragma library

function indexOfValue(values, target, fallback) {
    for (var i = 0; i < values.length; ++i) if (values[i] === target) {
        return i;
    }
    return fallback;
}
