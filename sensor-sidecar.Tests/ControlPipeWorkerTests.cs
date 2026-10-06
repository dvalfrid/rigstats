using System.IO.Pipes;
using System.Text.Json;
using NSubstitute;
using SensorSidecar.Control;
using SensorSidecar.Control.Lighting;
using SensorSidecar.Control.Providers;
using Xunit;

namespace SensorSidecar.Tests;

/// <summary>
/// The control pipe end to end (#226): the real <see cref="ControlPipeWorker"/>
/// on a per-test pipe name, driven by a <see cref="NamedPipeClientStream"/> —
/// the accept loop, the verifier gate, request/response correlation, the event
/// pump sharing the writer, and clients leaving mid-preview. Fake fan headers,
/// a temp profile store and a substitute verifier: no hardware, no elevation.
/// </summary>
public sealed class ControlPipeWorkerTests : IAsyncLifetime
{
    private static readonly TimeSpan ReadTimeout = TimeSpan.FromSeconds(5);

    private readonly string _dir = Path.Combine(Path.GetTempPath(), $"rigstats-controlpipe-{Guid.NewGuid():N}");
    private readonly string _pipeName = $"rigstats-control-test-{Guid.NewGuid():N}";
    private readonly IPipeClientVerifier _verifier = Substitute.For<IPipeClientVerifier>();
    private readonly IControlProvider _powerPlan = Substitute.For<IControlProvider>();
    private readonly FanProvider _fans;
    private readonly FanCurveLoop _fanLoop;
    private readonly BootCrashGuard _crashGuard;
    private readonly HueLink _hue;
    private readonly ControlPipeWorker _worker;

    public ControlPipeWorkerTests()
    {
        Directory.CreateDirectory(_dir);
        _verifier.Verify(Arg.Any<NamedPipeServerStream>()).Returns(VerifyResult.Allow());
        _powerPlan.Domain.Returns("power_plan");
        _powerPlan.Validate(Arg.Any<ProfilePart>()).Returns(ValidationResult.Success());
        _powerPlan.Capture().Returns(new Snapshot { Domain = "power_plan" });
        _powerPlan.Verify(Arg.Any<ProfilePart>()).Returns(true);

        var host = new FakeHardwareHost(FanSamples.Board(2));
        _fans = new FanProvider(host, dryRun: false);
        _fanLoop = new FanCurveLoop(host, _fans);
        _crashGuard = new BootCrashGuard(Path.Combine(_dir, "crash-guard"), BootCrashGuard.DefaultStableAfter);
        _hue = new HueLink(Path.Combine(_dir, "hue.json"));
        IControlProvider[] providers = [_powerPlan, _fans];
        _worker = new ControlPipeWorker(
            new ControlBroker(providers),
            new ProfileStore(Path.Combine(_dir, "profiles.json")),
            new SafetyGuard(providers, dryRun: false),
            providers,
            _verifier,
            _fans,
            _fanLoop,
            _crashGuard,
            new LightingProvider([], "no devices", () => null, dryRun: true),
            _hue,
            _pipeName);
    }

    public Task InitializeAsync() => _worker.StartAsync(CancellationToken.None);

    public async Task DisposeAsync()
    {
        using (var cts = new CancellationTokenSource(TimeSpan.FromSeconds(10)))
            await _worker.StopAsync(cts.Token);
        _worker.Dispose();
        _crashGuard.Dispose();
        _hue.Dispose();
        try { Directory.Delete(_dir, recursive: true); } catch { }
    }

    private sealed class Client(NamedPipeClientStream pipe) : IDisposable
    {
        private readonly StreamReader _reader = new(pipe);
        private readonly StreamWriter _writer = new(pipe) { AutoFlush = true };
        private long _nextId = 1;

        /// Not awaited by callers that expect the server not to be reading yet.
        public Task SendAsync(string line) => _writer.WriteLineAsync(line);

        public async Task<string?> ReadLineAsync(TimeSpan? timeout = null)
        {
            using var cts = new CancellationTokenSource(timeout ?? ReadTimeout);
            return await _reader.ReadLineAsync(cts.Token);
        }

        public async Task<JsonElement> ReadJsonAsync(TimeSpan? timeout = null)
        {
            var line = await ReadLineAsync(timeout) ?? throw new EndOfStreamException("pipe closed");
            return JsonDocument.Parse(line).RootElement.Clone();
        }

        /// Sends a request and reads lines until its response (events on the
        /// way are skipped).
        public async Task<JsonElement> RequestAsync(string method, object? parameters = null)
        {
            var id = _nextId++;
            await SendAsync(JsonSerializer.Serialize(new { id, method, @params = parameters }, ControlJson.Options));
            while (true)
            {
                var message = await ReadJsonAsync();
                if (message.TryGetProperty("id", out var got) && got.GetInt64() == id)
                    return message;
            }
        }

        public void Dispose() => pipe.Dispose();
    }

    private async Task<Client> ConnectAsync()
    {
        var pipe = new NamedPipeClientStream(".", _pipeName, PipeDirection.InOut, PipeOptions.Asynchronous);
        try
        {
            await pipe.ConnectAsync((int)ReadTimeout.TotalMilliseconds);
        }
        catch
        {
            pipe.Dispose();
            throw;
        }
        return new Client(pipe);
    }

    private static async Task Eventually(Func<bool> condition, TimeSpan timeout)
    {
        var until = DateTime.UtcNow + timeout;
        while (!condition())
        {
            if (DateTime.UtcNow > until)
                throw new TimeoutException("condition not met in time");
            await Task.Delay(50);
        }
    }

    [Fact]
    public async Task A_provider_whose_probe_throws_is_unavailable_and_the_others_still_listed()
    {
        _powerPlan.Probe().Returns(_ => throw new InvalidOperationException("driver gone"));
        using var client = await ConnectAsync();
        await client.RequestAsync("hello", new { protocol = 1 });

        var response = await client.RequestAsync("capabilities");

        var caps = response.GetProperty("result").EnumerateArray().ToList();
        var power = caps.Single(c => c.GetProperty("domain").GetString() == "power_plan");
        Assert.False(power.GetProperty("supported").GetBoolean());
        Assert.Contains(caps, c => c.GetProperty("domain").GetString() == "fan");
    }

    private static Profile PreviewProfile() =>
        new() { Id = "balanced", Name = "Balanced", Part = new ProfilePart { PowerPlan = "high_performance" } };

    [Fact]
    public async Task Hello_reports_the_protocol_and_list_profiles_the_builtins()
    {
        using var client = await ConnectAsync();

        var hello = await client.RequestAsync("hello", new { protocol = 1 });
        Assert.Equal(1, hello.GetProperty("result").GetProperty("protocol").GetInt32());
        Assert.False(string.IsNullOrEmpty(hello.GetProperty("result").GetProperty("service_version").GetString()));

        var profiles = await client.RequestAsync("list_profiles");
        var ids = profiles.GetProperty("result").EnumerateArray().Select(p => p.GetProperty("id").GetString());
        Assert.Equal(["silent", "balanced", "gaming", "eco"], ids);
    }

    [Fact]
    public async Task Malformed_json_is_a_bad_request_and_the_connection_stays_usable()
    {
        using var client = await ConnectAsync();

        await client.SendAsync("{ not json");
        var bad = await client.ReadJsonAsync();
        Assert.Equal("bad_request", bad.GetProperty("error").GetProperty("code").GetString());

        var hello = await client.RequestAsync("hello");
        Assert.True(hello.TryGetProperty("result", out _));
    }

    [Fact]
    public async Task An_unknown_method_is_answered_with_unknown_method()
    {
        using var client = await ConnectAsync();

        var response = await client.RequestAsync("no_such_method");

        Assert.Equal("unknown_method", response.GetProperty("error").GetProperty("code").GetString());
    }

    [Fact]
    public async Task A_denied_client_is_closed_unanswered_and_the_next_one_is_served()
    {
        _verifier.Verify(Arg.Any<NamedPipeServerStream>())
            .Returns(VerifyResult.Deny("not the installed rigstats.exe"), VerifyResult.Allow());

        using (var denied = await ConnectAsync())
            Assert.Null(await denied.ReadLineAsync());

        using var allowed = await ConnectAsync();
        var hello = await allowed.RequestAsync("hello");
        Assert.True(hello.TryGetProperty("result", out _));
    }

    [Fact]
    public async Task Fan_events_arrive_only_after_subscribe()
    {
        _fans.Apply(new ProfilePart
        {
            Fan = new FanPart
            {
                Headers = new() { [FanSamples.HeaderId(0)] = new FanHeaderConfig { Source = "cpu_package", Curve = [[40, 30], [80, 70]] } },
            },
        });
        using var client = await ConnectAsync();
        await client.RequestAsync("hello");

        // Not subscribed: a tick sends nothing, so the very next line is the
        // response to the request after it (read raw — RequestAsync skips events).
        await _fanLoop.TickAsync(CancellationToken.None);
        await client.SendAsync("""{"id":99,"method":"hello"}""");
        var next = await client.ReadJsonAsync();
        Assert.False(next.TryGetProperty("event", out _));
        Assert.Equal(99, next.GetProperty("id").GetInt64());

        await client.RequestAsync("subscribe");
        await _fanLoop.TickAsync(CancellationToken.None);
        var message = await client.ReadJsonAsync();
        Assert.Equal("fan_duty", message.GetProperty("event").GetString());
        Assert.True(message.GetProperty("data").GetProperty("duty").TryGetProperty(FanSamples.HeaderId(0), out _));
    }

    [Fact]
    public async Task A_preview_without_confirm_reverts_and_says_so()
    {
        using var client = await ConnectAsync();
        await client.RequestAsync("subscribe");

        var preview = await client.RequestAsync("preview", new { profile = PreviewProfile(), seconds = 1 });
        Assert.True(preview.GetProperty("result").GetProperty("ok").GetBoolean());

        var reverted = await client.ReadJsonAsync();
        Assert.Equal("preview_reverted", reverted.GetProperty("event").GetString());
        _powerPlan.Received(1).Restore(Arg.Any<Snapshot>());
    }

    [Fact]
    public async Task A_client_leaving_during_a_preview_still_reverts_it_and_a_new_client_is_served()
    {
        using (var client = await ConnectAsync())
        {
            var preview = await client.RequestAsync("preview", new { profile = PreviewProfile(), seconds = 1 });
            Assert.True(preview.GetProperty("result").GetProperty("ok").GetBoolean());
        }

        await Eventually(() => _powerPlan.ReceivedCalls().Any(c => c.GetMethodInfo().Name == nameof(IControlProvider.Restore)),
            TimeSpan.FromSeconds(5));

        using var next = await ConnectAsync();
        var hello = await next.RequestAsync("hello");
        Assert.True(hello.TryGetProperty("result", out _));
    }

    [Fact]
    public async Task A_second_client_waits_until_the_first_leaves()
    {
        var first = await ConnectAsync();
        await first.RequestAsync("hello");

        using var second = await ConnectAsync();
        _ = second.SendAsync("""{"id":1,"method":"hello"}""");
        await Assert.ThrowsAnyAsync<OperationCanceledException>(() => second.ReadLineAsync(TimeSpan.FromSeconds(1)));

        first.Dispose();
        var hello = await second.ReadJsonAsync();
        Assert.Equal(1, hello.GetProperty("id").GetInt64());
        Assert.True(hello.TryGetProperty("result", out _));
    }
}
