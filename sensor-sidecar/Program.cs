using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Hosting;
using SensorSidecar;

var builder = Host.CreateApplicationBuilder(args);
builder.Services.AddWindowsService(options =>
{
    options.ServiceName = "RIGStats Sensor";
});
builder.Services.AddHostedService<SensorWorker>();

var host = builder.Build();
host.Run();
