using SensorSidecar.Control;
using SensorSidecar.Control.Lighting;
using Xunit;

namespace SensorSidecar.Tests;

/// <see cref="SteelSeriesBaseStation"/>: the Arctis Nova Pro Wireless's
/// battery from its base station (#621) — one documented question, `06 B0`,
/// and only its own reply read.
public class SteelSeriesBaseStationTests
{
    /// Writes recorded; replies handed out in order, then nothing.
    private sealed class Station(params byte[][] replies) : IHidDevice
    {
        private readonly Queue<byte[]> _replies = new(replies);
        public List<byte[]> Written { get; } = [];
        public void Write(byte[] report) => Written.Add((byte[])report.Clone());
        public byte[]? Read(TimeSpan timeout) => _replies.Count > 0 ? _replies.Dequeue() : null;
        public void Dispose() { }
    }

    /// A status reply: `06 B0`, the level at byte 6, the status at byte 15.
    private static byte[] Status(byte level, byte power)
    {
        var reply = new byte[64];
        reply[0] = 0x06;
        reply[1] = 0xB0;
        reply[6] = level;
        reply[15] = power;
        return reply;
    }

    // The tester's base station (1.52.0-rc.3 diagnostics): interface 4 has
    // two vendor collections; report id 6 is on FFC0's.
    private static readonly HidDeviceInfo[] TesterScan =
    [
        new(@"\?\hid#vid_1038&pid_12e0&mi_03#a", 0x1038, 0x12E0, 0x000C, 0x0001, 0, 2, 0, "Arctis Nova Pro Wireless"),
        new(@"\?\hid#vid_1038&pid_12e0&mi_04&col01#b", 0x1038, 0x12E0, 0xFF00, 0x0001, 64, 64, 64, "Arctis Nova Pro Wireless", 7),
        new(@"\?\hid#vid_1038&pid_12e0&mi_04&col02#c", 0x1038, 0x12E0, 0xFFC0, 0x0001, 64, 64, 1024, "Arctis Nova Pro Wireless", 6),
    ];

    [Fact]
    public void The_status_request_goes_to_the_collection_with_report_id_6()
    {
        var station = Assert.Single(SteelSeriesBaseStation.Stations(TesterScan));
        Assert.Equal(0xFFC0, station.UsagePage);
    }

    [Fact]
    public void Only_06_B0_is_sent_padded_to_the_report_length()
    {
        var hid = new Station(Status(6, 0x08));
        SteelSeriesBaseStation.Read(hid, 64);

        var request = Assert.Single(hid.Written);
        Assert.Equal(64, request.Length);
        Assert.Equal([0x06, 0xB0], request[..2]);
        Assert.All(request[2..], b => Assert.Equal(0, b));
    }

    [Theory]
    [InlineData(8, 0x08, 100, false)]
    [InlineData(6, 0x08, 75, false)]
    [InlineData(1, 0x08, 13, false)]
    [InlineData(4, 0x02, 50, true)]
    public void The_level_is_eighths_and_02_is_charging(byte level, byte power, int percent, bool charging)
    {
        Assert.Equal(new BatteryStatus(percent, charging), SteelSeriesBaseStation.Battery(Status(level, power)));
    }

    [Fact]
    public void An_offline_headset_or_a_level_out_of_range_reads_as_no_answer()
    {
        Assert.Null(SteelSeriesBaseStation.Battery(Status(6, 0x01)));
        Assert.Null(SteelSeriesBaseStation.Battery(Status(9, 0x08)));
    }

    [Fact]
    public void Another_programs_reply_is_skipped()
    {
        // SteelSeries GG's traffic reaches our handle too: a volume report
        // (07 25) comes first, then our answer.
        var other = new byte[64];
        other[0] = 0x07;
        other[1] = 0x25;
        other[6] = 2;
        var hid = new Station(other, Status(6, 0x08));

        Assert.Equal(new BatteryStatus(75, false), SteelSeriesBaseStation.Read(hid, 64));
        Assert.Single(hid.Written);
    }

    [Fact]
    public void A_base_station_is_asked_only_after_its_start_up()
    {
        var stations = new SteelSeriesBaseStations();
        var plugged = new DateTime(2026, 10, 25, 20, 0, 0, DateTimeKind.Utc);

        Assert.Empty(stations.Devices(TesterScan, plugged));
        Assert.Empty(stations.Devices(TesterScan, plugged + TimeSpan.FromSeconds(4)));
        var device = Assert.Single(stations.Devices(TesterScan, plugged + SteelSeriesBaseStation.Settle));
        Assert.Equal("SteelSeries Arctis Nova Pro Wireless", device.Name);
        Assert.Equal("headset", device.Kind);
    }
}
