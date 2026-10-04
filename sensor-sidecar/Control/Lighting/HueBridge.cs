using System.Net;
using System.Net.Http.Json;
using System.Net.Security;
using System.Net.Sockets;
using System.Security.Cryptography.X509Certificates;
using System.Text;
using System.Text.Json.Nodes;

namespace SensorSidecar.Control.Lighting;

/// The bridge didn't answer (off, unplugged, or moved to another address).
public sealed class HueUnreachableException(string message, Exception inner) : Exception(message, inner);

/// A Hue Bridge as found on the network (from its unauthenticated config).
public sealed record HueBridgeInfo(string Id, string Name, string Model, string Firmware, string Ip);

/// A room or zone on the bridge and the grouped light that switches it.
public sealed record HueGroup(string Id, string Name, string Kind, string GroupedLightId);

/// The Hue Bridge's official local API (#215): found by mDNS, paired with
/// the link button, driven over CLIP v2 — no cloud, no OpenRGB. HTTPS only,
/// and only to a bridge whose certificate is signed by Philips Hue / Signify
/// and names the bridge we paired with (the bridge id is its CN); never
/// with validation turned off.
public sealed class HueBridge : IDisposable
{
    private static readonly TimeSpan RequestTimeout = TimeSpan.FromSeconds(3);

    /// The bridge accepts about one grouped-light command a second; faster
    /// ones are dropped, so they are spaced here instead.
    private static readonly TimeSpan GroupCommandSpacing = TimeSpan.FromSeconds(1);

    private readonly HttpClient _http;
    private readonly string _key;
    private readonly object _sendLock = new();
    private readonly TlsCheck _tls = new();
    private long _lastGroupCommand = long.MinValue / 2;

    public HueBridge(string ip, string bridgeId, string key)
    {
        Ip = ip;
        BridgeId = bridgeId;
        _key = key;
        _http = Client(bridgeId, _tls);
    }

    public string Ip { get; }
    public string BridgeId { get; }

    /// The rooms and zones, sorted by name.
    public async Task<IReadOnlyList<HueGroup>> GroupsAsync(CancellationToken ct)
    {
        var groups = new List<HueGroup>();
        foreach (var kind in new[] { "room", "zone" })
        {
            var data = await Call(Ip, _tls, async () =>
            {
                using var request = Request(HttpMethod.Get, $"clip/v2/resource/{kind}");
                using var response = await _http.SendAsync(request, ct);
                return await ReadAsync(response, ct);
            });
            foreach (var item in data)
            {
                if (item?["id"]?.GetValue<string>() is not { } id
                    || item["services"]?.AsArray().FirstOrDefault(s => s?["rtype"]?.GetValue<string>() == "grouped_light")?["rid"]?.GetValue<string>() is not { } light)
                    continue;
                groups.Add(new HueGroup(id, item["metadata"]?["name"]?.GetValue<string>() ?? kind, kind, light));
            }
        }
        return groups.OrderBy(g => g.Name, StringComparer.CurrentCultureIgnoreCase).ToList();
    }

    /// One command to a room's or zone's lights (`on`, `dimming`, `color`,
    /// `dynamics`). Waits its turn so the bridge doesn't drop it; throws
    /// when the bridge is unreachable or refuses.
    public void SetGroup(string groupedLightId, JsonObject body)
    {
        lock (_sendLock)
        {
            var wait = _lastGroupCommand + (long)GroupCommandSpacing.TotalMilliseconds - Environment.TickCount64;
            if (wait > 0)
                Thread.Sleep((int)wait);
            _lastGroupCommand = Environment.TickCount64;
            Call(Ip, _tls, () =>
            {
                using var request = Request(HttpMethod.Put, $"clip/v2/resource/grouped_light/{groupedLightId}");
                request.Content = new StringContent(body.ToJsonString(), Encoding.UTF8, "application/json");
                using var response = _http.Send(request);
                return Task.FromResult(ReadAsync(response, CancellationToken.None).GetAwaiter().GetResult());
            }).GetAwaiter().GetResult();
        }
    }

    public void Dispose() => _http.Dispose();

    private HttpRequestMessage Request(HttpMethod method, string path)
    {
        var request = new HttpRequestMessage(method, $"https://{Ip}/{path}");
        request.Headers.Add("hue-application-key", _key);
        return request;
    }

    /// CLIP v2's `data`, or its first error as the exception.
    private static async Task<JsonArray> ReadAsync(HttpResponseMessage response, CancellationToken ct)
    {
        if (response.StatusCode is HttpStatusCode.Forbidden or HttpStatusCode.Unauthorized)
            throw new InvalidOperationException("The Hue Bridge no longer knows RIGStats — pair it again.");
        var body = await response.Content.ReadFromJsonAsync<JsonObject>(ct);
        if (body?["errors"]?.AsArray().FirstOrDefault()?["description"]?.GetValue<string>() is { } error)
            throw new InvalidOperationException($"Hue Bridge: {error}");
        response.EnsureSuccessStatusCode();
        return body?["data"]?.AsArray() ?? [];
    }

    // ── Discovery and pairing ───────────────────────────────────────────

    /// Every bridge that answers on the local network, or only `ip` when
    /// given (the fallback when mDNS is blocked).
    public static async Task<IReadOnlyList<HueBridgeInfo>> DiscoverAsync(string? ip, CancellationToken ct)
    {
        var addresses = ip is not null ? [ip] : (await MdnsAsync(TimeSpan.FromMilliseconds(1200), ct)).Select(a => a.ToString()).ToList();
        var found = await Task.WhenAll(addresses.Distinct().Select(async a =>
        {
            try
            {
                return await ConfigAsync(a, ct);
            }
            catch (Exception e) when (ip is null)
            {
                SidecarLog.Log($"[rigstats-control] Hue: {a} answered mDNS but not as a bridge: {e.Message}");
                return null;
            }
        }));
        return found.OfType<HueBridgeInfo>().ToList();
    }

    /// The bridge's own description — no key needed. Throws when `ip` isn't
    /// a Hue Bridge with a genuine certificate.
    public static async Task<HueBridgeInfo> ConfigAsync(string ip, CancellationToken ct)
    {
        var tls = new TlsCheck();
        using var http = Client(null, tls);
        var config = await Call(ip, tls, () => http.GetFromJsonAsync<JsonObject>($"https://{ip}/api/0/config", ct))
            ?? throw new InvalidOperationException($"{ip} sent no bridge config.");
        var id = config["bridgeid"]?.GetValue<string>()
            ?? throw new InvalidOperationException($"{ip} is not a Hue Bridge.");
        if (!string.Equals(tls.CommonName, id, StringComparison.OrdinalIgnoreCase))
            throw new InvalidOperationException($"{ip}'s certificate is not for bridge {id}.");
        return new HueBridgeInfo(
            id.ToLowerInvariant(),
            config["name"]?.GetValue<string>() ?? "Hue Bridge",
            config["modelid"]?.GetValue<string>() ?? "",
            config["swversion"]?.GetValue<string>() ?? "",
            ip);
    }

    /// Asks the bridge for an application key. Null while the link button
    /// hasn't been pressed (the bridge allows 30 s after a press).
    public static async Task<string?> PairAsync(HueBridgeInfo bridge, CancellationToken ct)
    {
        var tls = new TlsCheck();
        using var http = Client(bridge.Id, tls);
        var device = Environment.MachineName.Length > 19 ? Environment.MachineName[..19] : Environment.MachineName;
        var reply = await Call(bridge.Ip, tls, async () =>
        {
            using var response = await http.PostAsJsonAsync($"https://{bridge.Ip}/api",
                new JsonObject { ["devicetype"] = $"rigstats#{device}" }, ct);
            return (await response.Content.ReadFromJsonAsync<JsonArray>(ct))?.FirstOrDefault();
        });
        if (reply?["success"]?["username"]?.GetValue<string>() is { } key)
            return key;
        if (reply?["error"]?["type"]?.GetValue<int>() == LinkButtonNotPressed)
            return null;
        throw new InvalidOperationException($"Hue Bridge: {reply?["error"]?["description"]?.GetValue<string>() ?? "pairing failed"}");
    }

    private const int LinkButtonNotPressed = 101;

    /// One legacy-unicast mDNS question for `_hue._tcp.local` (RFC 6762
    /// §6.7: asked from a port other than 5353, so every bridge answers
    /// straight back to it — no multicast group to join, nothing for the
    /// firewall to open). The bridges are the hosts that answer.
    private static async Task<IReadOnlyList<IPAddress>> MdnsAsync(TimeSpan window, CancellationToken ct)
    {
        using var udp = new UdpClient(new IPEndPoint(IPAddress.Any, 0));
        await udp.SendAsync(MdnsQuery, new IPEndPoint(IPAddress.Parse("224.0.0.251"), 5353), ct);
        using var timeout = CancellationTokenSource.CreateLinkedTokenSource(ct);
        timeout.CancelAfter(window);
        var found = new List<IPAddress>();
        try
        {
            while (true)
            {
                var reply = await udp.ReceiveAsync(timeout.Token);
                if (IsHueAnswer(reply.Buffer) && !found.Contains(reply.RemoteEndPoint.Address))
                    found.Add(reply.RemoteEndPoint.Address);
            }
        }
        catch (OperationCanceledException) when (!ct.IsCancellationRequested)
        {
            // The window is over.
        }
        catch (SocketException e)
        {
            SidecarLog.Log($"[rigstats-control] Hue: mDNS search failed: {e.Message}");
        }
        return found;
    }

    /// A DNS query: id 0, one question, PTR `_hue._tcp.local`, class IN.
    internal static readonly byte[] MdnsQuery =
    [
        0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0,
        4, (byte)'_', (byte)'h', (byte)'u', (byte)'e',
        4, (byte)'_', (byte)'t', (byte)'c', (byte)'p',
        5, (byte)'l', (byte)'o', (byte)'c', (byte)'a', (byte)'l', 0,
        0, 12, 0, 1,
    ];

    /// A DNS response that names `_hue` — a legacy-unicast answer repeats
    /// the question, so the label is always there uncompressed.
    internal static bool IsHueAnswer(byte[] packet) =>
        packet.Length > 12 && (packet[2] & 0x80) != 0
        && packet.AsSpan(12).IndexOf("\u0004_hue"u8) >= 0;

    // ── TLS ────────────────────────────────────────────────────────────

    /// What the certificate check saw on the last connection: the CN, and
    /// why the certificate was refused (null when it wasn't) — HttpClient
    /// itself only says "the SSL connection could not be established".
    private sealed class TlsCheck
    {
        public string? CommonName;
        public string? Refusal;
    }

    /// `bridgeId` (or, when null, any bridge id — left in `check` for the
    /// caller to compare) must be the certificate's CN, and the certificate
    /// must chain to a Hue root. The host name never matches: bridges are
    /// reached by IP.
    private static HttpClient Client(string? bridgeId, TlsCheck check)
    {
        var handler = new SocketsHttpHandler
        {
            ConnectTimeout = RequestTimeout,
            SslOptions = new SslClientAuthenticationOptions
            {
                RemoteCertificateValidationCallback = (_, certificate, presented, _) =>
                {
                    check.Refusal = CertificateRefusal(certificate as X509Certificate2, presented, bridgeId, out check.CommonName);
                    return check.Refusal is null;
                },
            },
        };
        return new HttpClient(handler) { Timeout = RequestTimeout };
    }

    /// Why a bridge's certificate is refused, or null when it is accepted.
    internal static string? CertificateRefusal(X509Certificate2? leaf, X509Chain? presented, string? bridgeId, out string? commonName)
    {
        commonName = null;
        if (leaf is null)
            return "it sent no certificate";
        if (!ChainsToHueRoot(leaf, presented))
            return "its certificate isn't signed by Philips Hue or Signify — not a genuine Hue Bridge, or its firmware needs an update in the Hue app";
        commonName = leaf.GetNameInfo(X509NameType.SimpleName, forIssuer: false);
        if (bridgeId is not null && !string.Equals(commonName, bridgeId, StringComparison.OrdinalIgnoreCase))
            return $"it is bridge {commonName}, not the paired bridge {bridgeId}";
        return null;
    }

    /// Runs one request to the bridge at `ip`. A refused certificate, a
    /// timeout or no connection becomes an error that says which.
    private static async Task<T> Call<T>(string ip, TlsCheck tls, Func<Task<T>> call)
    {
        try
        {
            return await call();
        }
        catch (HttpRequestException e) when (tls.Refusal is { } refusal)
        {
            throw new InvalidOperationException($"Refused the device at {ip}: {refusal}.", e);
        }
        catch (HttpRequestException e) when (e.HttpRequestError == HttpRequestError.ConnectionError)
        {
            throw new HueUnreachableException($"The Hue Bridge at {ip} can't be reached ({e.Message}).", e);
        }
        catch (TaskCanceledException e) when (e.InnerException is TimeoutException)
        {
            throw new HueUnreachableException($"The Hue Bridge at {ip} didn't answer within {RequestTimeout.TotalSeconds:0} s.", e);
        }
    }

    internal static bool ChainsToHueRoot(X509Certificate2 leaf, X509Chain? presented)
    {
        using var chain = new X509Chain();
        chain.ChainPolicy.TrustMode = X509ChainTrustMode.CustomRootTrust;
        chain.ChainPolicy.RevocationMode = X509RevocationMode.NoCheck;
        chain.ChainPolicy.CustomTrustStore.AddRange(Roots.Value);
        if (presented is not null)
        {
            foreach (var element in presented.ChainElements)
                chain.ChainPolicy.ExtraStore.Add(element.Certificate);
        }
        return chain.Build(leaf);
    }

    /// Philips Hue's bridge root, and Signify's newer one (2025) — the CAs
    /// every genuine bridge certificate is signed by, as published for the
    /// Hue API's HTTPS guidance.
    private static readonly Lazy<X509Certificate2Collection> Roots = new(() =>
    {
        var roots = new X509Certificate2Collection();
        roots.ImportFromPem(RootsPem);
        return roots;
    });

    private const string RootsPem = """
        -----BEGIN CERTIFICATE-----
        MIICMjCCAdigAwIBAgIUO7FSLbaxikuXAljzVaurLXWmFw4wCgYIKoZIzj0EAwIw
        OTELMAkGA1UEBhMCTkwxFDASBgNVBAoMC1BoaWxpcHMgSHVlMRQwEgYDVQQDDAty
        b290LWJyaWRnZTAiGA8yMDE3MDEwMTAwMDAwMFoYDzIwMzgwMTE5MDMxNDA3WjA5
        MQswCQYDVQQGEwJOTDEUMBIGA1UECgwLUGhpbGlwcyBIdWUxFDASBgNVBAMMC3Jv
        b3QtYnJpZGdlMFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEjNw2tx2AplOf9x86
        aTdvEcL1FU65QDxziKvBpW9XXSIcibAeQiKxegpq8Exbr9v6LBnYbna2VcaK0G22
        jOKkTqOBuTCBtjAPBgNVHRMBAf8EBTADAQH/MA4GA1UdDwEB/wQEAwIBhjAdBgNV
        HQ4EFgQUZ2ONTFrDT6o8ItRnKfqWKnHFGmQwdAYDVR0jBG0wa4AUZ2ONTFrDT6o8
        ItRnKfqWKnHFGmShPaQ7MDkxCzAJBgNVBAYTAk5MMRQwEgYDVQQKDAtQaGlsaXBz
        IEh1ZTEUMBIGA1UEAwwLcm9vdC1icmlkZ2WCFDuxUi22sYpLlwJY81Wrqy11phcO
        MAoGCCqGSM49BAMCA0gAMEUCIEBYYEOsa07TH7E5MJnGw557lVkORgit2Rm1h3B2
        sFgDAiEA1Fj/C3AN5psFMjo0//mrQebo0eKd3aWRx+pQY08mk48=
        -----END CERTIFICATE-----
        -----BEGIN CERTIFICATE-----
        MIIBzDCCAXOgAwIBAgICEAAwCgYIKoZIzj0EAwIwPDELMAkGA1UEBhMCTkwxFDAS
        BgNVBAoMC1NpZ25pZnkgSHVlMRcwFQYDVQQDDA5IdWUgUm9vdCBDQSAwMTAgFw0y
        NTAyMjUwMDAwMDBaGA8yMDUwMTIzMTIzNTk1OVowPDELMAkGA1UEBhMCTkwxFDAS
        BgNVBAoMC1NpZ25pZnkgSHVlMRcwFQYDVQQDDA5IdWUgUm9vdCBDQSAwMTBZMBMG
        ByqGSM49AgEGCCqGSM49AwEHA0IABFfOO0jfSAUXGQ9kjEDzyBrcMQ3ItyA5krE+
        cyvb1Y3xFti7KlAad8UOnAx0FBLn7HZrlmIwm1QnX0fK3LPM13mjYzBhMB0GA1Ud
        DgQWBBTF1pSpsCASX/z0VHLigxU2CAaqoTAfBgNVHSMEGDAWgBTF1pSpsCASX/z0
        VHLigxU2CAaqoTAPBgNVHRMBAf8EBTADAQH/MA4GA1UdDwEB/wQEAwIBBjAKBggq
        hkjOPQQDAgNHADBEAiAk7duT+IHbOGO4UUuGLAEpyYejGZK9Z7V9oSfnvuQ5BQIg
        IYSgwwxHXm73/JgcU9lAM6c8Bmu3UE3kBIUwBs1qXFw=
        -----END CERTIFICATE-----
        """;
}
