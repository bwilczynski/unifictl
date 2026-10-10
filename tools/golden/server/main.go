// Command unifictl-golden serves homelab-api's network endpoints from the
// UniFi fixtures in $FIXTURE_DIR on 127.0.0.1:$PORT. tools/golden/generate.sh
// compiles it inside the homelab-api module with `go build -overlay`, so it
// can use homelab-api's internal packages without adding a file there.
package main

import (
	"context"
	"log"
	"log/slog"
	"net/http"
	"os"

	"github.com/go-chi/chi/v5"

	"github.com/bwilczynski/homelab-api/internal/adapters"
	"github.com/bwilczynski/homelab-api/internal/apierrors"
	"github.com/bwilczynski/homelab-api/internal/network"
	"github.com/bwilczynski/homelab-api/internal/routing"
	"github.com/bwilczynski/homelab-api/internal/testhelpers"
)

type fixtureBackend struct {
	devices  []adapters.UniFiDevice
	clients  []adapters.UniFiSta
	active   []adapters.UniFiClientV2
	offline  []adapters.UniFiClientV2
	wlans    []adapters.UniFiWlanConf
	networks []adapters.UniFiNetworkConf
}

func (b *fixtureBackend) GetDevices(context.Context) ([]adapters.UniFiDevice, error) {
	return b.devices, nil
}

func (b *fixtureBackend) GetClients(context.Context) ([]adapters.UniFiSta, error) {
	return b.clients, nil
}

func (b *fixtureBackend) GetActiveClients(context.Context) ([]adapters.UniFiClientV2, error) {
	return b.active, nil
}

func (b *fixtureBackend) GetOfflineClients(context.Context, int) ([]adapters.UniFiClientV2, error) {
	return b.offline, nil
}

func (b *fixtureBackend) GetAllClients(context.Context, int) ([]adapters.UniFiClientV2, error) {
	return append(append([]adapters.UniFiClientV2{}, b.active...), b.offline...), nil
}

func (b *fixtureBackend) GetWlanConf(context.Context) ([]adapters.UniFiWlanConf, error) {
	return b.wlans, nil
}

func (b *fixtureBackend) GetNetworkConf(context.Context) ([]adapters.UniFiNetworkConf, error) {
	return b.networks, nil
}

func main() {
	dir := os.Getenv("FIXTURE_DIR")
	b := &fixtureBackend{
		devices:  testhelpers.MustLoadFixture[[]adapters.UniFiDevice](dir + "/unifi-devices.json"),
		clients:  testhelpers.MustLoadFixture[[]adapters.UniFiSta](dir + "/unifi-clients.json"),
		active:   testhelpers.MustLoadFixture[[]adapters.UniFiClientV2](dir + "/unifi-v2-active.json"),
		offline:  testhelpers.MustLoadFixture[[]adapters.UniFiClientV2](dir + "/unifi-v2-history.json"),
		wlans:    testhelpers.MustLoadFixture[[]adapters.UniFiWlanConf](dir + "/unifi-wlanconf.json"),
		networks: testhelpers.MustLoadFixture[[]adapters.UniFiNetworkConf](dir + "/unifi-networkconf.json"),
	}
	svc := network.NewService(map[string]network.UniFiBackend{"unifi": b}, map[string]int{"unifi": 30}, slog.Default(), nil)

	r := chi.NewRouter()
	r.Use(routing.EscapedPathRouting)
	network.HandlerWithOptions(network.NewStrictHandler(network.NewHandler(svc), nil), network.ChiServerOptions{
		BaseRouter:       r,
		ErrorHandlerFunc: apierrors.ProblemBadRequestHandler,
	})
	log.Fatal(http.ListenAndServe("127.0.0.1:"+os.Getenv("PORT"), r))
}
