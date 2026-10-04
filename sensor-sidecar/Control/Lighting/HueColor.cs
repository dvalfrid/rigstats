namespace SensorSidecar.Control.Lighting;

/// RGB → what a Hue light takes (#215): a CIE xy colour point and a
/// brightness in percent. Pure maths, unit-tested.
public static class HueColor
{
    /// Hue's gamut C (every current colour bulb and strip); a point outside
    /// it is moved to the nearest one inside, which the bridge would do too —
    /// doing it here keeps what is sent what is shown.
    private static readonly (double X, double Y) Red = (0.6915, 0.3083);
    private static readonly (double X, double Y) Green = (0.17, 0.7);
    private static readonly (double X, double Y) Blue = (0.1532, 0.0475);

    /// D65, the white of sRGB — where black and grey land.
    public static readonly (double X, double Y) White = (0.3127, 0.3290);

    /// The colour's xy and its brightness (0–100, the brightest channel —
    /// the colour arrives already scaled by the profile's brightness).
    public static (double X, double Y, double Brightness) FromRgb(byte red, byte green, byte blue)
    {
        var brightness = Math.Max(red, Math.Max(green, blue)) / 255.0 * 100.0;
        var (r, g, b) = (Linear(red), Linear(green), Linear(blue));
        // sRGB (D65) → CIE XYZ.
        var x = r * 0.4124 + g * 0.3576 + b * 0.1805;
        var y = r * 0.2126 + g * 0.7152 + b * 0.0722;
        var z = r * 0.0193 + g * 0.1192 + b * 0.9505;
        var sum = x + y + z;
        if (sum <= 0)
            return (White.X, White.Y, 0);
        var (cx, cy) = Clamp((x / sum, y / sum));
        return (Math.Round(cx, 4), Math.Round(cy, 4), Math.Round(brightness, 1));
    }

    private static double Linear(byte channel)
    {
        var c = channel / 255.0;
        return c > 0.04045 ? Math.Pow((c + 0.055) / 1.055, 2.4) : c / 12.92;
    }

    /// The point itself when inside gamut C, else the nearest point on its edge.
    public static (double X, double Y) Clamp((double X, double Y) p)
    {
        if (Inside(p))
            return p;
        var candidates = new[] { Nearest(Red, Green, p), Nearest(Green, Blue, p), Nearest(Blue, Red, p) };
        return candidates.MinBy(c => Distance(c, p));
    }

    private static bool Inside((double X, double Y) p)
    {
        var d1 = Side(p, Red, Green);
        var d2 = Side(p, Green, Blue);
        var d3 = Side(p, Blue, Red);
        var negative = d1 < 0 || d2 < 0 || d3 < 0;
        var positive = d1 > 0 || d2 > 0 || d3 > 0;
        return !(negative && positive);
    }

    private static double Side((double X, double Y) p, (double X, double Y) a, (double X, double Y) b) =>
        (p.X - b.X) * (a.Y - b.Y) - (a.X - b.X) * (p.Y - b.Y);

    private static (double X, double Y) Nearest((double X, double Y) a, (double X, double Y) b, (double X, double Y) p)
    {
        var (dx, dy) = (b.X - a.X, b.Y - a.Y);
        var t = Math.Clamp(((p.X - a.X) * dx + (p.Y - a.Y) * dy) / (dx * dx + dy * dy), 0, 1);
        return (a.X + t * dx, a.Y + t * dy);
    }

    private static double Distance((double X, double Y) a, (double X, double Y) b) =>
        Math.Sqrt((a.X - b.X) * (a.X - b.X) + (a.Y - b.Y) * (a.Y - b.Y));
}
