using System.ComponentModel;
using SensorSidecar.Control.Providers;
using Xunit;

namespace SensorSidecar.Tests;

/// `RyzenSmu` itself against a fake PawnIO module (#680): the module calls,
/// RSMU command ids and arguments it sends, its retries and how it handles
/// a rejected reply. `CpuLimitProviderTests` / `CurveOptimizerProviderTests`
/// replace `RyzenSmu` entirely; this pins what sits below them. Expected ids
/// come from `AmdSmuMap` and docs/control-architecture.md (Phase 2 / 4).
public class RyzenSmuTests
{
    private const uint Verified = 0x620105; // 9800X3D, the only verified layout.
    private const int StatusInternalError = 1359; // the module's 0xFD.

    // The service's Global\Access_PCI is created by LocalSystem and closed to
    // a normal user, so the tests lock a mutex of their own.
    private static readonly string TestMutex = $@"Local\rigstats-ryzensmu-test-{Guid.NewGuid():N}";

    /// Records every call; answers the PM table functions from its fields and
    /// `ioctl_send_smu_command` from `Smu`. A thrown `Win32Exception` is how
    /// the real module reports a rejected command.
    private sealed class FakeModule : IPawnIoModule
    {
        public List<(string Fn, long[] Input, int OutCount)> Calls { get; } = [];
        public uint TableVersion { get; set; } = Verified;
        public long[] Table { get; } = new long[64];
        public Exception? ResolveError { get; set; }
        /// Error codes for successive `ioctl_update_pm_table` calls; 0 = ok.
        public Queue<int> UpdateErrors { get; } = new();
        public Func<long[], long>? Smu { get; set; }
        public bool Disposed { get; private set; }

        public IEnumerable<long[]> SmuCommands =>
            Calls.Where(c => c.Fn == "ioctl_send_smu_command").Select(c => c.Input);

        public long[] Execute(string fn, long[] input, int outCount)
        {
            Calls.Add((fn, input.ToArray(), outCount));
            var reply = new long[outCount];
            switch (fn)
            {
                case "ioctl_resolve_pm_table":
                    if (ResolveError is not null)
                        throw ResolveError;
                    reply[0] = TableVersion;
                    break;
                case "ioctl_update_pm_table":
                    if (UpdateErrors.TryDequeue(out var error) && error != 0)
                        throw new Win32Exception(error);
                    break;
                case "ioctl_read_pm_table":
                    Array.Copy(Table, reply, Math.Min(outCount, Table.Length));
                    break;
                case "ioctl_send_smu_command":
                    reply[0] = Smu?.Invoke(input) ?? 0;
                    break;
                default:
                    throw new InvalidOperationException($"unexpected module function {fn}");
            }
            return reply;
        }

        public void SetFloat(int index, float value)
        {
            var bits = (long)(uint)BitConverter.SingleToInt32Bits(value);
            var q = Table[index / 2];
            Table[index / 2] = index % 2 == 0
                ? (q & unchecked((long)0xFFFFFFFF00000000)) | bits
                : (q & 0xFFFFFFFF) | (bits << 32);
        }

        public void Dispose() => Disposed = true;
    }

    private static RyzenSmu Open(FakeModule module, string generation = "Granite Ridge")
    {
        var commands = generation is "Matisse" or "Vermeer"
            ? new SmuLimitCommands(0x53, 0x54, 0x55)
            : new SmuLimitCommands(0x56, 0x57, 0x58);
        var smu = RyzenSmu.Open(module, generation, commands, "test cpu", out var reason, TestMutex);
        Assert.NotNull(smu);
        Assert.Equal("", reason);
        module.Calls.Clear();
        return smu;
    }

    /// The module functions called, in order, without the `ioctl_` prefix.
    private static string Fns(FakeModule module) =>
        string.Join(" ", module.Calls.Select(c => c.Fn.Replace("ioctl_", "")));

    private static long[] Command(uint id, uint arg) => [id, arg, 0, 0, 0, 0, 0];

    // ── Open: the PM table version gate ─────────────────────────────────

    [Fact]
    public void Open_on_a_verified_table_only_resolves_the_table()
    {
        var module = new FakeModule();

        var smu = RyzenSmu.Open(module, "Granite Ridge", new SmuLimitCommands(0x56, 0x57, 0x58), "cpu", out var reason, TestMutex);

        Assert.NotNull(smu);
        Assert.Equal("", reason);
        Assert.Equal("Granite Ridge", smu.Generation);
        Assert.Equal(Verified, smu.PmTableVersion);
        var call = Assert.Single(module.Calls);
        Assert.Equal("ioctl_resolve_pm_table", call.Fn);
        Assert.Empty(call.Input);
        Assert.Equal(2, call.OutCount);
        Assert.False(module.Disposed);
    }

    [Fact]
    public void Open_on_an_unknown_table_only_reads_its_head_and_sends_no_command()
    {
        var module = new FakeModule { TableVersion = 0x540104 };

        var smu = RyzenSmu.Open(module, "Granite Ridge", new SmuLimitCommands(0x56, 0x57, 0x58), "cpu", out var reason, TestMutex);

        Assert.Null(smu);
        Assert.Contains("0x540104", reason);
        Assert.Equal("resolve_pm_table update_pm_table read_pm_table", Fns(module));
        Assert.Equal(64, module.Calls[2].OutCount); // 128 floats for the log.
        Assert.Empty(module.SmuCommands);
        Assert.True(module.Disposed);
    }

    [Fact]
    public void Open_when_the_smu_does_not_answer_gives_up_without_a_command()
    {
        var module = new FakeModule { ResolveError = new Win32Exception(StatusInternalError) };

        var smu = RyzenSmu.Open(module, "Granite Ridge", new SmuLimitCommands(0x56, 0x57, 0x58), "cpu", out var reason, TestMutex);

        Assert.Null(smu);
        Assert.Contains("did not answer", reason);
        Assert.Single(module.Calls);
        Assert.Empty(module.SmuCommands);
        Assert.True(module.Disposed);
    }

    // ── CPU limits ──────────────────────────────────────────────────────

    [Theory]
    [InlineData(0x17u, 0x71u, 0u, 0x53u, 0x54u, 0x55u)] // Matisse
    [InlineData(0x19u, 0x21u, 0u, 0x53u, 0x54u, 0x55u)] // Vermeer
    [InlineData(0x19u, 0x61u, 0u, 0x56u, 0x57u, 0x58u)] // Raphael
    [InlineData(0x1Au, 0x44u, 0u, 0x56u, 0x57u, 0x58u)] // Granite Ridge
    public void SetLimits_sends_the_generations_ppt_tdc_edc_ids_in_milli_units(
        uint family, uint model, uint pkg, uint ppt, uint tdc, uint edc)
    {
        var (name, commands) = AmdSmuMap.Generation(family, model, pkg)!.Value;
        var module = new FakeModule();
        var smu = RyzenSmu.Open(module, name, commands, "cpu", out _, TestMutex)!;
        module.Calls.Clear();

        smu.SetLimits(new AmdLimits(88, 60, 90.5));

        Assert.Equal(new[] { Command(ppt, 88_000), Command(tdc, 60_000), Command(edc, 90_500) }, module.SmuCommands);
        Assert.All(module.Calls, c => Assert.Equal(6, c.OutCount));
    }

    [Fact]
    public void SetLimits_stops_at_the_first_rejected_command_without_retrying()
    {
        var module = new FakeModule();
        var smu = Open(module);
        module.Smu = input => input[0] == 0x57 ? throw new Win32Exception(StatusInternalError) : 0;

        var e = Assert.Throws<Win32Exception>(() => smu.SetLimits(new AmdLimits(100, 80, 120)));

        Assert.Equal(StatusInternalError, e.NativeErrorCode);
        // PPT is already written; EDC is never sent. The caller's verify /
        // revert has to deal with the half-applied state.
        Assert.Equal("56 57", string.Join(" ", module.SmuCommands.Select(c => $"{c[0]:X}")));
    }

    [Fact]
    public void ReadLimits_transfers_the_table_then_reads_the_limit_floats()
    {
        var module = new FakeModule();
        module.SetFloat(2, 162f);
        module.SetFloat(8, 120f);
        module.SetFloat(63, 180f);
        var smu = Open(module);

        var limits = smu.ReadLimits();

        Assert.Equal(new AmdLimits(162, 120, 180), limits);
        Assert.Equal("update_pm_table read_pm_table", Fns(module));
        Assert.Equal(32, module.Calls[1].OutCount); // floats 0..63, two per value.
        Assert.Empty(module.SmuCommands);
    }

    [Fact]
    public void ReadLimits_resolves_the_table_again_after_a_rejected_transfer()
    {
        var module = new FakeModule();
        module.SetFloat(2, 95f);
        var smu = Open(module);
        module.UpdateErrors.Enqueue(StatusInternalError);
        module.UpdateErrors.Enqueue(StatusInternalError);

        var limits = smu.ReadLimits();

        Assert.Equal(95, limits.PptW);
        Assert.Equal("update_pm_table resolve_pm_table update_pm_table resolve_pm_table update_pm_table read_pm_table", Fns(module));
    }

    [Theory]
    [InlineData(1, false)] // Two attempts: the usual case on the 9800X3D.
    [InlineData(2, true)]
    public void A_retried_transfer_is_logged_only_above_two_attempts(int rejections, bool logged)
    {
        var module = new FakeModule();
        var smu = Open(module);
        for (var i = 0; i < rejections; i++)
            module.UpdateErrors.Enqueue(StatusInternalError);

        var before = Console.Error;
        var captured = new StringWriter();
        Console.SetError(captured);
        try
        {
            smu.ReadLimits();
        }
        finally
        {
            Console.SetError(before);
        }

        Assert.Equal(logged, captured.ToString().Contains("PM table transfer needed"));
    }

    [Fact]
    public void ReadLimits_gives_up_after_five_rejected_transfers()
    {
        var module = new FakeModule();
        var smu = Open(module);
        for (var i = 0; i < 10; i++)
            module.UpdateErrors.Enqueue(StatusInternalError);

        var e = Assert.Throws<Win32Exception>(() => smu.ReadLimits());

        Assert.Equal(StatusInternalError, e.NativeErrorCode);
        Assert.Equal(5, module.Calls.Count(c => c.Fn == "ioctl_update_pm_table"));
        Assert.Equal(4, module.Calls.Count(c => c.Fn == "ioctl_resolve_pm_table"));
        Assert.DoesNotContain(module.Calls, c => c.Fn == "ioctl_read_pm_table");
    }

    [Fact]
    public void ReadLimits_does_not_retry_any_other_error()
    {
        var module = new FakeModule();
        var smu = Open(module);
        module.UpdateErrors.Enqueue(31); // ERROR_GEN_FAILURE

        Assert.Throws<Win32Exception>(() => smu.ReadLimits());

        Assert.Equal("update_pm_table", Fns(module));
    }

    // ── Curve Optimizer ─────────────────────────────────────────────────

    [Theory]
    [InlineData("Matisse")]
    [InlineData("Vermeer")]
    [InlineData("Raphael")]
    public void Curve_optimizer_is_not_offered_and_never_sent_on_an_unverified_generation(string generation)
    {
        var module = new FakeModule();
        var smu = Open(module, generation);

        Assert.Null(smu.CurveOptimizer);
        Assert.Throws<InvalidOperationException>(() => smu.SetAllOffsets(-10));
        Assert.Throws<InvalidOperationException>(() => smu.SetOffset(new PhysicalCore(0, 0), -10));
        Assert.Throws<InvalidOperationException>(() => smu.GetOffset(new PhysicalCore(0, 0)));
        Assert.Throws<InvalidOperationException>(() => smu.Cores()); // before the first probe.
        Assert.Empty(module.Calls);
    }

    [Fact]
    public void Curve_optimizer_is_offered_on_granite_ridge()
    {
        Assert.NotNull(Open(new FakeModule()).CurveOptimizer);
    }

    [Fact]
    public void SetAllOffsets_sends_set_all_with_the_offset_in_16_bit_twos_complement()
    {
        var module = new FakeModule();
        var smu = Open(module);

        smu.SetAllOffsets(-20);

        Assert.Equal(new[] { Command(0x7, 0x0000FFEC) }, module.SmuCommands);
    }

    [Fact]
    public void SetOffset_sends_set_per_core_with_ccd_and_core_in_the_top_bits()
    {
        var module = new FakeModule();
        var smu = Open(module);

        smu.SetOffset(new PhysicalCore(1, 3), -15);
        smu.SetOffset(new PhysicalCore(0, 7), 0);

        Assert.Equal(new[] { Command(0x6, 0x1030FFF1), Command(0x6, 0x00700000) }, module.SmuCommands);
    }

    [Fact]
    public void GetOffset_asks_get_for_the_core_and_reads_the_low_16_bits_signed()
    {
        var module = new FakeModule { Smu = _ => 0x0050FFF6 }; // junk above bit 15 is ignored.
        var smu = Open(module);

        var offset = smu.GetOffset(new PhysicalCore(0, 5));

        Assert.Equal(-10, offset);
        Assert.Equal(new[] { Command(0xD5, 0x00500000) }, module.SmuCommands);
    }

    [Fact]
    public void Cores_probes_two_ccds_of_eight_and_keeps_the_ones_that_answer()
    {
        // As on the 9800X3D: CCD 0 answers, CCD 1 is rejected.
        var module = new FakeModule
        {
            Smu = input => (input[1] >> 28) == 1 ? throw new Win32Exception(StatusInternalError) : 0,
        };
        var smu = Open(module);

        var cores = smu.Cores();

        Assert.Equal(Enumerable.Range(0, 8).Select(c => new PhysicalCore(0, c)), cores);
        var probes = module.SmuCommands.ToList();
        Assert.Equal(16, probes.Count);
        Assert.All(probes, p => Assert.Equal(0xD5L, p[0])); // only GetDldoPsmMargin, never a set.
        Assert.Equal(
            Enumerable.Range(0, 2).SelectMany(ccd => Enumerable.Range(0, 8).Select(core => (long)AmdSmuMap.CoreMask(ccd, core))),
            probes.Select(p => p[1]));
    }

    [Fact]
    public void Cores_is_asked_once_and_then_cached()
    {
        var module = new FakeModule();
        var smu = Open(module);

        var first = smu.Cores();
        var second = smu.Cores();

        Assert.Same(first, second);
        Assert.Equal(16, module.SmuCommands.Count());
    }

    [Fact]
    public void Cores_counts_any_rejected_probe_as_an_absent_core()
    {
        // A transient rejection on a core that exists drops it too — and the
        // list is cached for the life of the handle (current behaviour).
        var module = new FakeModule
        {
            Smu = input => (input[1] >> 28) == 1 || input[1] == AmdSmuMap.CoreMask(0, 3)
                ? throw new Win32Exception(StatusInternalError)
                : 0,
        };
        var smu = Open(module);

        Assert.Equal(7, smu.Cores().Count);
        Assert.DoesNotContain(new PhysicalCore(0, 3), smu.Cores());
    }

    [Fact]
    public void A_held_pci_mutex_times_out_without_sending_anything()
    {
        var module = new FakeModule();
        var smu = Open(module);
        using var held = new ManualResetEventSlim();
        using var release = new ManualResetEventSlim();
        var holder = new Thread(() =>
        {
            using var other = new Mutex(false, TestMutex);
            other.WaitOne();
            held.Set();
            release.Wait();
            other.ReleaseMutex();
        });
        holder.Start();
        held.Wait();
        try
        {
            Assert.Throws<TimeoutException>(() => smu.SetLimits(new AmdLimits(100, 80, 120)));
            Assert.Empty(module.Calls);
        }
        finally
        {
            release.Set();
            holder.Join();
        }
    }

    [Fact]
    public void Dispose_releases_the_module()
    {
        var module = new FakeModule();
        var smu = Open(module);

        smu.Dispose();

        Assert.True(module.Disposed);
    }
}
