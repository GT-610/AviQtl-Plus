#pragma once
#include <QColor>
#include <QHash>
#include <QVariant>
#include <QVariantList>
#include <QVariantMap>
#include <algorithm>
#include <cmath>
#include <functional>
#include <vector>

namespace AviQtl::Core::KeyframeUtils {

using EasingFunction = std::function<double(double, const std::vector<double> &, const QVariantMap &)>;

// Pre-extracted track point to avoid per-frame QVariantMap allocations
struct TrackPoint {
    int frame;
    QVariant value;
    QString interp;
    QVariantMap modeParams;
};

inline std::vector<TrackPoint> extractTrackPoints(const QVariantList &track) {
    std::vector<TrackPoint> points;
    points.reserve(track.size());
    for (const auto &v : track) {
        const QVariantMap m = v.toMap();
        points.push_back({
            m.value(QStringLiteral("frame")).toInt(),
            m.value(QStringLiteral("value")),
            m.value(QStringLiteral("interp")).toString(),
            m.value(QStringLiteral("modeParams")).toMap()
        });
    }
    return points;
}

inline QVariant evaluateTrackFast(const std::vector<TrackPoint> &track, int frame, const QVariant &fallback) {
    if (track.empty())
        return fallback;

    if (frame <= track.front().frame)
        return track.front().value;
    if (frame >= track.back().frame)
        return track.back().value;

    const bool numeric = fallback.canConvert<double>();

    // Binary search for the correct segment
    size_t lo = 0, hi = track.size() - 1;
    while (lo < hi - 1) {
        size_t mid = lo + (hi - lo) / 2;
        if (track[mid].frame <= frame)
            lo = mid;
        else
            hi = mid;
    }

    const auto &p0 = track[lo];
    const auto &p1 = track[lo + 1];
    const int f0 = p0.frame, f1 = p1.frame;
    const QVariant &v0 = p0.value, &v1 = p1.value;

    if (f0 == f1)
        return v0;
    const double tRaw = (frame - f0) / double(f1 - f0);

    if (p0.interp == QStringLiteral("none"))
        return (frame < f1) ? v0 : v1;

    if (v0.typeId() == QMetaType::QString && v1.typeId() == QMetaType::QString) {
        QColor c0(v0.toString()), c1(v1.typeId() == QMetaType::QString ? v1.toString() : v0.toString());
        if (c0.isValid() && c1.isValid()) {
            std::vector<double> params;
            if (p0.interp == QStringLiteral("custom")) {
                params = {p0.modeParams.value(QStringLiteral("bzx1"), 0.33).toDouble(), p0.modeParams.value(QStringLiteral("bzy1"), 0.0).toDouble(),
                          p0.modeParams.value(QStringLiteral("bzx2"), 0.66).toDouble(), p0.modeParams.value(QStringLiteral("bzy2"), 1.0).toDouble(), 1.0, 1.0};
            }
            const auto &funcs = easingFunctions();
            QString type = p0.interp;
            auto efIt = funcs.find(type);
            if (efIt == funcs.end()) { type = QStringLiteral("linear"); efIt = funcs.find(type); }
            const double t = efIt.value()(tRaw, params, p0.modeParams);
            return QColor(static_cast<int>(c0.red() + (c1.red() - c0.red()) * t), static_cast<int>(c0.green() + (c1.green() - c0.green()) * t),
                          static_cast<int>(c0.blue() + (c1.blue() - c0.blue()) * t), static_cast<int>(c0.alpha() + (c1.alpha() - c0.alpha()) * t))
                .name(QColor::HexArgb);
        }
    }

    if (!numeric || !v0.canConvert<double>() || !v1.canConvert<double>())
        return v0;

    const double a = v0.toDouble(), b = v1.toDouble();
    if (p0.interp == QStringLiteral("random")) {
        const int stepFrames = std::max(1, p0.modeParams.value(QStringLiteral("stepFrames"), 1).toInt()), stepIndex = (frame - f0) / stepFrames;
        const quint32 seed = qHash(f0) ^ qHash(f1) ^ qHash(stepIndex) ^ qHash(static_cast<qint64>(a * 1000)) ^ qHash(static_cast<qint64>(b * 1000));
        return std::min(a, b) + (std::max(a, b) - std::min(a, b)) * (double(seed % 1000000u) / 999999.0);
    }
    if (p0.interp == QStringLiteral("alternate")) {
        const int stepFrames = std::max(1, p0.modeParams.value(QStringLiteral("stepFrames"), 1).toInt());
        return ((frame - f0) / stepFrames % 2 == 0) ? a : b;
    }

    std::vector<double> params;
    if (p0.interp == QStringLiteral("custom")) {
        params = {p0.modeParams.value(QStringLiteral("bzx1"), 0.33).toDouble(), p0.modeParams.value(QStringLiteral("bzy1"), 0.0).toDouble(),
                  p0.modeParams.value(QStringLiteral("bzx2"), 0.66).toDouble(), p0.modeParams.value(QStringLiteral("bzy2"), 1.0).toDouble(), 1.0, 1.0};
    }
    const auto &funcs = easingFunctions();
    QString type = p0.interp;
    auto efIt = funcs.find(type);
    if (efIt == funcs.end()) { type = QStringLiteral("linear"); efIt = funcs.find(type); }
    return a + (b - a) * efIt.value()(tRaw, params, p0.modeParams);
}

inline bool isStructuredTrack(const QVariant &raw) {
    const QVariantMap m = raw.toMap();
    return m.contains(QStringLiteral("start")) && m.contains(QStringLiteral("points"));
}

inline QVariantList sortPoints(QVariantList points) {
    std::sort(points.begin(), points.end(), [](const QVariant &a, const QVariant &b) {
        return a.toMap().value(QStringLiteral("frame")).toInt() < b.toMap().value(QStringLiteral("frame")).toInt();
    });
    return points;
}

inline int inferredDurationForTrack(const QVariant &raw) {
    if (isStructuredTrack(raw)) {
        const QVariantList points = raw.toMap().value(QStringLiteral("points")).toList();
        int maxFrame = 0;
        for (const auto &v : std::as_const(points))
            maxFrame = std::max(maxFrame, v.toMap().value(QStringLiteral("frame")).toInt());
        return std::max(1, maxFrame + 1);
    }
    const QVariantList list = raw.toList();
    if (list.isEmpty())
        return 1;
    int maxFrame = 0;
    for (const auto &v : std::as_const(list))
        maxFrame = std::max(maxFrame, v.toMap().value(QStringLiteral("frame")).toInt());
    return std::max(1, maxFrame + 1);
}

inline QVariantList flattenStructuredTrack(const QVariantMap &track) {
    QVariantList out;
    out.append(track.value(QStringLiteral("start")));
    QVariantList points = track.value(QStringLiteral("points")).toList();
    points = sortPoints(points);
    for (const auto &v : std::as_const(points))
        out.append(v);
    return out;
}

inline double solveBezierT(double x, double x1, double x2) {
    if (x1 == x2 && x1 == x)
        return x;
    double t = x;
    for (int i = 0; i < 8; ++i) {
        const double one_minus_t = 1.0 - t;
        const double current_x = 3 * one_minus_t * one_minus_t * t * x1 + 3 * one_minus_t * t * t * x2 + t * t * t;
        const double error = current_x - x;
        if (std::abs(error) < 1e-5)
            return t;
        const double dx_dt = 3 * one_minus_t * one_minus_t * x1 + 6 * one_minus_t * t * (x2 - x1) + 3 * t * t * (1.0 - x2);
        if (std::abs(dx_dt) < 1e-6)
            break;
        t -= error / dx_dt;
    }
    return std::clamp(t, 0.0, 1.0);
}

inline const QHash<QString, EasingFunction> &easingFunctions() {
    static auto easeOutBounce = [](double x) -> double {
        constexpr double n1 = 7.5625, d1 = 2.75;
        if (x < 1.0 / d1) return n1 * x * x;
        if (x < 2.0 / d1) { x -= 1.5 / d1; return n1 * x * x + 0.75; }
        if (x < 2.5 / d1) { x -= 2.25 / d1; return n1 * x * x + 0.9375; }
        x -= 2.625 / d1;
        return n1 * x * x + 0.984375;
    };

    static const QHash<QString, EasingFunction> funcs = {
        {QStringLiteral("linear"), [](double t, const auto &, const auto &) { return t; }},
        {QStringLiteral("ease_in_sine"), [](double t, const auto &, const auto &) { return 1.0 - std::cos(t * M_PI / 2.0); }},
        {QStringLiteral("ease_out_sine"), [](double t, const auto &, const auto &) { return std::sin(t * M_PI / 2.0); }},
        {QStringLiteral("ease_in_out_sine"), [](double t, const auto &, const auto &) { return -(std::cos(M_PI * t) - 1.0) / 2.0; }},
        {QStringLiteral("ease_out_in_sine"), [](double t, const auto &, const auto &) { return t < 0.5 ? std::sin(t * M_PI) / 2.0 : (1.0 - std::cos((t * 2.0 - 1.0) * M_PI / 2.0)) / 2.0 + 0.5; }},
        {QStringLiteral("ease_in_quad"), [](double t, const auto &, const auto &) { return t * t; }},
        {QStringLiteral("ease_out_quad"), [](double t, const auto &, const auto &) { return 1.0 - (1.0 - t) * (1.0 - t); }},
        {QStringLiteral("ease_in_out_quad"), [](double t, const auto &, const auto &) { return t < 0.5 ? 2.0 * t * t : 1.0 - ((-2.0 * t + 2.0) * (-2.0 * t + 2.0)) / 2.0; }},
        {QStringLiteral("ease_out_in_quad"), [](double t, const auto &, const auto &) { return t < 0.5 ? (1.0 - (1.0 - 2.0 * t) * (1.0 - 2.0 * t)) / 2.0 : (2.0 * t - 1.0) * (2.0 * t - 1.0) / 2.0 + 0.5; }},
        {QStringLiteral("ease_in_cubic"), [](double t, const auto &, const auto &) { return t * t * t; }},
        {QStringLiteral("ease_out_cubic"), [](double t, const auto &, const auto &) { return 1.0 - (1.0 - t) * (1.0 - t) * (1.0 - t); }},
        {QStringLiteral("ease_in_out_cubic"), [](double t, const auto &, const auto &) { return t < 0.5 ? 4.0 * t * t * t : 1.0 - ((-2.0 * t + 2.0) * (-2.0 * t + 2.0) * (-2.0 * t + 2.0)) / 2.0; }},
        {QStringLiteral("ease_out_in_cubic"), [](double t, const auto &, const auto &) { return t < 0.5 ? (1.0 - std::pow(1.0 - 2.0 * t, 3.0)) / 2.0 : std::pow(2.0 * t - 1.0, 3.0) / 2.0 + 0.5; }},
        {QStringLiteral("ease_in_quart"), [](double t, const auto &, const auto &) { return t * t * t * t; }},
        {QStringLiteral("ease_out_quart"), [](double t, const auto &, const auto &) { return 1.0 - (1.0 - t) * (1.0 - t) * (1.0 - t) * (1.0 - t); }},
        {QStringLiteral("ease_in_out_quart"), [](double t, const auto &, const auto &) { return t < 0.5 ? 8.0 * t * t * t * t : 1.0 - ((-2.0 * t + 2.0) * (-2.0 * t + 2.0) * (-2.0 * t + 2.0) * (-2.0 * t + 2.0)) / 2.0; }},
        {QStringLiteral("ease_out_in_quart"), [](double t, const auto &, const auto &) { return t < 0.5 ? (1.0 - std::pow(1.0 - 2.0 * t, 4.0)) / 2.0 : std::pow(2.0 * t - 1.0, 4.0) / 2.0 + 0.5; }},
        {QStringLiteral("ease_in_quint"), [](double t, const auto &, const auto &) { return t * t * t * t * t; }},
        {QStringLiteral("ease_out_quint"), [](double t, const auto &, const auto &) { return 1.0 - (1.0 - t) * (1.0 - t) * (1.0 - t) * (1.0 - t) * (1.0 - t); }},
        {QStringLiteral("ease_in_out_quint"), [](double t, const auto &, const auto &) { return t < 0.5 ? 16.0 * t * t * t * t * t : 1.0 - ((-2.0 * t + 2.0) * (-2.0 * t + 2.0) * (-2.0 * t + 2.0) * (-2.0 * t + 2.0) * (-2.0 * t + 2.0)) / 2.0; }},
        {QStringLiteral("ease_out_in_quint"), [](double t, const auto &, const auto &) { return t < 0.5 ? (1.0 - std::pow(1.0 - 2.0 * t, 5.0)) / 2.0 : std::pow(2.0 * t - 1.0, 5.0) / 2.0 + 0.5; }},
        {QStringLiteral("ease_in_expo"), [](double t, const auto &, const auto &) { return t == 0.0 ? 0.0 : std::pow(2.0, 10.0 * t - 10.0); }},
        {QStringLiteral("ease_out_expo"), [](double t, const auto &, const auto &) { return t == 1.0 ? 1.0 : 1.0 - std::pow(2.0, -10.0 * t); }},
        {QStringLiteral("ease_in_out_expo"), [](double t, const auto &, const auto &) {
            if (t == 0.0) return 0.0; if (t == 1.0) return 1.0;
            return t < 0.5 ? std::pow(2.0, 20.0 * t - 10.0) / 2.0 : (2.0 - std::pow(2.0, -20.0 * t + 10.0)) / 2.0;
        }},
        {QStringLiteral("ease_out_in_expo"), [](double t, const auto &, const auto &) {
            if (t == 0.0) return 0.0; if (t == 1.0) return 1.0;
            return t < 0.5 ? (1.0 - std::pow(2.0, -20.0 * t)) / 2.0 : std::pow(2.0, 20.0 * t - 20.0) / 2.0 + 0.5;
        }},
        {QStringLiteral("ease_in_circ"), [](double t, const auto &, const auto &) { return 1.0 - std::sqrt(1.0 - t * t); }},
        {QStringLiteral("ease_out_circ"), [](double t, const auto &, const auto &) { return std::sqrt(1.0 - (t - 1.0) * (t - 1.0)); }},
        {QStringLiteral("ease_in_out_circ"), [](double t, const auto &, const auto &) {
            return t < 0.5 ? (1.0 - std::sqrt(1.0 - 4.0 * t * t)) / 2.0 : (std::sqrt(1.0 - (-2.0 * t + 2.0) * (-2.0 * t + 2.0)) + 1.0) / 2.0;
        }},
        {QStringLiteral("ease_out_in_circ"), [](double t, const auto &, const auto &) { return t < 0.5 ? std::sqrt(1.0 - (2.0 * t - 1.0) * (2.0 * t - 1.0)) / 2.0 : (1.0 - std::sqrt(1.0 - (2.0 * t - 1.0) * (2.0 * t - 1.0))) / 2.0 + 0.5; }},
        {QStringLiteral("ease_in_back"), [](double t, const auto &, const auto &) {
            constexpr double c1 = 1.70158, c3 = 1.70158 + 1.0;
            return c3 * t * t * t - c1 * t * t;
        }},
        {QStringLiteral("ease_out_back"), [](double t, const auto &, const auto &) {
            constexpr double c1 = 1.70158, c3 = 1.70158 + 1.0;
            return 1.0 + c3 * (t - 1.0) * (t - 1.0) * (t - 1.0) + c1 * (t - 1.0) * (t - 1.0);
        }},
        {QStringLiteral("ease_in_out_back"), [](double t, const auto &, const auto &) {
            constexpr double c2 = 1.70158 * 1.525;
            return t < 0.5 ? ((2.0 * t) * (2.0 * t) * ((c2 + 1.0) * 2.0 * t - c2)) / 2.0
                           : ((2.0 * t - 2.0) * (2.0 * t - 2.0) * ((c2 + 1.0) * (2.0 * t - 2.0) + c2) + 2.0) / 2.0;
        }},
        {QStringLiteral("ease_out_in_back"), [](double t, const auto &, const auto &) {
            constexpr double c1 = 1.70158, c3 = c1 + 1.0;
            auto eout = [&](double u) { return 1.0 + c3 * (u - 1.0) * (u - 1.0) * (u - 1.0) + c1 * (u - 1.0) * (u - 1.0); };
            auto ein = [&](double u) { return c3 * u * u * u - c1 * u * u; };
            return t < 0.5 ? eout(2.0 * t) / 2.0 : ein(2.0 * t - 1.0) / 2.0 + 0.5;
        }},
        {QStringLiteral("ease_in_elastic"), [](double t, const auto &, const auto &p) {
            double a = p.value("amplitude", 1.0).toDouble();
            double period = p.value("period", 0.3).toDouble();
            double c4 = (2.0 * M_PI) / period;
            if (t == 0.0) return 0.0; if (t == 1.0) return 1.0;
            return -a * std::pow(2.0, 10.0 * t - 10.0) * std::sin((t - 1.0 - period / 4.0) * c4);
        }},
        {QStringLiteral("ease_out_elastic"), [](double t, const auto &, const auto &p) {
            double a = p.value("amplitude", 1.0).toDouble();
            double period = p.value("period", 0.3).toDouble();
            double c4 = (2.0 * M_PI) / period;
            if (t == 0.0) return 0.0; if (t == 1.0) return 1.0;
            return a * std::pow(2.0, -10.0 * t) * std::sin((t - period / 4.0) * c4) + 1.0;
        }},
        {QStringLiteral("ease_in_out_elastic"), [](double t, const auto &, const auto &p) {
            double a = p.value("amplitude", 1.0).toDouble();
            double period = p.value("period", 0.3).toDouble() * 1.5;
            double c5 = (2.0 * M_PI) / period;
            if (t == 0.0) return 0.0; if (t == 1.0) return 1.0;
            return t < 0.5
                ? -(a * std::pow(2.0, 20.0 * t - 10.0) * std::sin((20.0 * t - 11.125) * c5)) / 2.0
                : (a * std::pow(2.0, -20.0 * t + 10.0) * std::sin((20.0 * t - 11.125) * c5)) / 2.0 + 1.0;
        }},
        {QStringLiteral("ease_out_in_elastic"), [](double t, const auto &, const auto &p) {
            double a = p.value("amplitude", 1.0).toDouble();
            double period = p.value("period", 0.3).toDouble();
            double c4 = (2.0 * M_PI) / period;
            if (t == 0.0) return 0.0; if (t == 1.0) return 1.0;
            auto eout = [&](double u) { return a * std::pow(2.0, -10.0 * u) * std::sin((u - period / 4.0) * c4) + 1.0; };
            auto ein = [&](double u) { return -a * std::pow(2.0, 10.0 * u - 10.0) * std::sin((u - 1.0 - period / 4.0) * c4); };
            return t < 0.5 ? eout(2.0 * t) / 2.0 : ein(2.0 * t - 1.0) / 2.0 + 0.5;
        }},
        {QStringLiteral("ease_out_bounce"), [](double t, const auto &, const auto &) { return easeOutBounce(t); }},
        {QStringLiteral("ease_in_bounce"), [](double t, const auto &, const auto &) { return 1.0 - easeOutBounce(1.0 - t); }},
        {QStringLiteral("ease_in_out_bounce"), [](double t, const auto &, const auto &) {
            return t < 0.5 ? (1.0 - easeOutBounce(1.0 - 2.0 * t)) / 2.0 : (1.0 + easeOutBounce(2.0 * t - 1.0)) / 2.0;
        }},
        {QStringLiteral("ease_out_in_bounce"), [](double t, const auto &, const auto &) {
            return t < 0.5 ? easeOutBounce(2.0 * t) / 2.0 : (1.0 - easeOutBounce(1.0 - 2.0 * (t - 0.5))) / 2.0 + 0.5;
        }},
        {QStringLiteral("custom"), [](double x, const auto &p, const auto &) {
            double prevX = 0, prevY = 0;
            for (size_t i = 0; i + 5 < p.size(); i += 6) {
                double cp1x = p[i], cp1y = p[i + 1], cp2x = p[i + 2], cp2y = p[i + 3], endX = p[i + 4], endY = p[i + 5];
                if (x <= endX || i + 6 >= p.size()) {
                    double range = endX - prevX;
                    if (range < 1e-6) return endY;
                    double n_cp1x = (cp1x - prevX) / range, n_cp2x = (cp2x - prevX) / range, n_x = (x - prevX) / range;
                    double t = solveBezierT(n_x, n_cp1x, n_cp2x);
                    return (1 - t) * (1 - t) * (1 - t) * prevY + 3 * (1 - t) * (1 - t) * t * cp1y + 3 * (1 - t) * t * t * cp2y + t * t * t * endY;
                }
                prevX = endX; prevY = endY;
            }
            return x;
        }}
    };
    return funcs;
}

inline QVariant evaluateTrack(const QVariantList &track, int frame, const QVariant &fallback) {
    if (track.isEmpty())
        return fallback;
    auto getFrame = [](const QVariant &v) { return v.toMap().value(QStringLiteral("frame")).toInt(); };
    auto getValue = [](const QVariant &v) { return v.toMap().value(QStringLiteral("value")); };

    if (frame <= getFrame(track.front()))
        return getValue(track.front());
    if (frame >= getFrame(track.back()))
        return getValue(track.back());

    const bool numeric = fallback.canConvert<double>();
    for (int i = 0; i < track.size() - 1; ++i) {
        const QVariantMap m_i = track[i].toMap();
        const QVariantMap m_i1 = track[i + 1].toMap();
        const int f0 = m_i.value(QStringLiteral("frame")).toInt(), f1 = m_i1.value(QStringLiteral("frame")).toInt();
        if (frame < f0 || frame > f1)
            continue;
        const QVariant v0 = m_i.value(QStringLiteral("value")), v1 = m_i1.value(QStringLiteral("value"));
        if (f0 == f1)
            return v0;
        const double tRaw = (frame - f0) / double(f1 - f0);
        QString type = m_i.value(QStringLiteral("interp")).toString();
        const QVariantMap modeParams = m_i.value(QStringLiteral("modeParams")).toMap();

        if (type == QStringLiteral("none"))
            return (frame < f1) ? v0 : v1;
        if (v0.typeId() == QMetaType::QString && v1.typeId() == QMetaType::QString) {
            QColor c0(v0.toString()), c1(v1.typeId() == QMetaType::QString ? v1.toString() : v0.toString());
            if (c0.isValid() && c1.isValid()) {
                std::vector<double> params;
                if (type == QStringLiteral("custom")) {
                    auto it = m_i.find(QStringLiteral("points"));
                    if (it != m_i.end()) {
                        QVariantList lst = it.value().toList();
                        for (const auto &val : std::as_const(lst))
                            params.push_back(val.toDouble());
                    } else {
                        params = {m_i.value(QStringLiteral("bzx1"), 0.33).toDouble(), m_i.value(QStringLiteral("bzy1"), 0.0).toDouble(),
                                  m_i.value(QStringLiteral("bzx2"), 0.66).toDouble(), m_i.value(QStringLiteral("bzy2"), 1.0).toDouble(), 1.0, 1.0};
                    }
                }
                const auto &funcs = easingFunctions();
                auto efIt = funcs.find(type);
                if (efIt == funcs.end()) { type = QStringLiteral("linear"); efIt = funcs.find(type); }
                const double t = efIt.value()(tRaw, params, modeParams);
                return QColor(static_cast<int>(c0.red() + (c1.red() - c0.red()) * t), static_cast<int>(c0.green() + (c1.green() - c0.green()) * t),
                              static_cast<int>(c0.blue() + (c1.blue() - c0.blue()) * t), static_cast<int>(c0.alpha() + (c1.alpha() - c0.alpha()) * t))
                    .name(QColor::HexArgb);
            }
        }
        if (!numeric || !v0.canConvert<double>() || !v1.canConvert<double>())
            return v0;
        const double a = v0.toDouble(), b = v1.toDouble();
        if (type == QStringLiteral("random")) {
            const int stepFrames = std::max(1, modeParams.value(QStringLiteral("stepFrames"), 1).toInt()), stepIndex = (frame - f0) / stepFrames;
            const quint32 seed = qHash(f0) ^ qHash(f1) ^ qHash(stepIndex) ^ qHash(static_cast<qint64>(a * 1000)) ^ qHash(static_cast<qint64>(b * 1000));
            return std::min(a, b) + (std::max(a, b) - std::min(a, b)) * (double(seed % 1000000u) / 999999.0);
        }
        if (type == QStringLiteral("alternate")) {
            const int stepFrames = std::max(1, modeParams.value(QStringLiteral("stepFrames"), 1).toInt());
            return ((frame - f0) / stepFrames % 2 == 0) ? a : b;
        }
        std::vector<double> params;
        if (type == QStringLiteral("custom")) {
            auto it = m_i.find(QStringLiteral("points"));
            if (it != m_i.end()) {
                QVariantList lst = it.value().toList();
                for (const auto &val : std::as_const(lst))
                    params.push_back(val.toDouble());
            } else {
                params = {m_i.value(QStringLiteral("bzx1"), 0.33).toDouble(), m_i.value(QStringLiteral("bzy1"), 0.0).toDouble(),
                          m_i.value(QStringLiteral("bzx2"), 0.66).toDouble(), m_i.value(QStringLiteral("bzy2"), 1.0).toDouble(), 1.0, 1.0};
            }
        }
        const auto &funcs = easingFunctions();
        auto efIt = funcs.find(type);
        if (efIt == funcs.end()) { type = QStringLiteral("linear"); efIt = funcs.find(type); }
        return a + (b - a) * efIt.value()(tRaw, params, modeParams);
    }
    return getValue(track.back());
}

inline QVariantMap normalizeTrackForDuration(const QVariant &rawTrack, const QVariant &fallback, int durationFrames) {
    if (isStructuredTrack(rawTrack)) {
        QVariantMap raw = rawTrack.toMap();
        QVariantMap start = raw.value(QStringLiteral("start")).toMap();
        QVariantList points = raw.value(QStringLiteral("points")).toList(), nextPoints;
        start[QStringLiteral("frame")] = 0;
        if (!start.contains(QStringLiteral("value")))
            start[QStringLiteral("value")] = fallback;

        const int ceiling = durationFrames;
        for (const auto &v : std::as_const(points)) {
            const int f = v.toMap().value(QStringLiteral("frame")).toInt();
            if (f > 0 && f <= ceiling)
                nextPoints.append(v);
        }
        QVariantMap out;
        out[QStringLiteral("start")] = start;
        out[QStringLiteral("points")] = sortPoints(nextPoints);
        return out;
    }
    QVariantList legacy = sortPoints(rawTrack.toList()), points;
    QVariantMap start;
    start[QStringLiteral("frame")] = 0;
    start[QStringLiteral("value")] = legacy.isEmpty() ? fallback : evaluateTrack(legacy, 0, fallback);
    // Preserve interp from existing frame-0 key if present, otherwise default to linear
    QString startInterp = QStringLiteral("linear");
    for (const auto &v : std::as_const(legacy)) {
        if (v.toMap().value(QStringLiteral("frame")).toInt() == 0) {
            startInterp = v.toMap().value(QStringLiteral("interp"), QStringLiteral("linear")).toString();
            break;
        }
    }
    start[QStringLiteral("interp")] = startInterp;
    for (const auto &v : std::as_const(legacy)) {
        const int f = v.toMap().value(QStringLiteral("frame")).toInt();
        if (f > 0 && f < durationFrames)
            points.append(v);
    }
    QVariantMap out;
    out[QStringLiteral("start")] = start;
    out[QStringLiteral("points")] = sortPoints(points);
    return out;
}

inline QVariant evaluateParam(const QVariantMap &params, const QVariantMap &keyframeTracks,
                              const QString &paramName, int frame, double fps, int durationFrames) {
    Q_UNUSED(fps);
    const QVariant fallback = params.value(paramName);
    auto ktIt = keyframeTracks.find(paramName);
    if (ktIt == keyframeTracks.end())
        return fallback;

    const QVariant raw = ktIt.value();
    QVariantList resolved;
    if (isStructuredTrack(raw)) {
        int d = (durationFrames > 0) ? durationFrames : inferredDurationForTrack(raw);
        QVariantMap normalized = normalizeTrackForDuration(raw, fallback, d);
        resolved = flattenStructuredTrack(normalized);
    } else {
        resolved = sortPoints(raw.toList());
    }

    return evaluateTrack(resolved, frame, fallback);
}

// Resolve one track to its flattened evaluation-ready form.
// This is the expensive step (normalize + flatten) and should be cached
// when evaluating many frames or many parameters of the same track.
inline QVariantList resolveTrack(const QVariant &raw, const QVariant &fallback, int durationFrames) {
    if (isStructuredTrack(raw)) {
        int d = (durationFrames > 0) ? durationFrames : inferredDurationForTrack(raw);
        QVariantMap normalized = normalizeTrackForDuration(raw, fallback, d);
        return flattenStructuredTrack(normalized);
    }
    return sortPoints(raw.toList());
}

// Resolve every keyframe track in a single pass. The returned hash maps each
// known parameter name (from `params` plus any track key) to its flattened
// evaluation-ready point list, sharing fallback values pulled from `params`.
// Callers can then call evaluateTrack() directly per frame without repeating
// the expensive normalize + flatten step.
inline QHash<QString, QVariantList> resolveAllTracks(const QVariantMap &params,
                                                     const QVariantMap &keyframeTracks,
                                                     int durationFrames) {
    QHash<QString, QVariantList> out;
    out.reserve(params.size() + keyframeTracks.size());
    for (auto it = keyframeTracks.constBegin(); it != keyframeTracks.constEnd(); ++it) {
        const QVariant fallback = params.value(it.key());
        out.insert(it.key(), resolveTrack(it.value(), fallback, durationFrames));
    }
    return out;
}

inline QVariant evaluateResolvedParam(const QVariantMap &params,
                                       const QHash<QString, QVariantList> &resolved,
                                       const QString &paramName, int frame) {
    const QVariant fallback = params.value(paramName);
    auto it = resolved.find(paramName);
    if (it == resolved.end())
        return fallback;
    return evaluateTrack(it.value(), frame, fallback);
}

} // namespace AviQtl::Core::KeyframeUtils
