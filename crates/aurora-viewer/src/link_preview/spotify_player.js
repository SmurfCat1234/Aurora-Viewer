(() => {
    const mount = document.getElementById("spotify-player");
    const fallback = document.getElementById("spotify-fallback");
    let committed = false;
    const showFallback = () => {
        if (committed) return;
        committed = true;
        document.body.replaceChildren(fallback.content.cloneNode(true));
    };
    const timeout = setTimeout(showFallback, 6000);

    window.onSpotifyIframeApiReady = (api) => {
        if (committed) return;
        try {
            api.createController(mount, { uri: mount.dataset.uri, width: "100%", height: "100%" }, (controller) => {
                controller.addListener("ready", () => {
                    if (committed) return;
                    committed = true;
                    clearTimeout(timeout);
                    controller.play();
                });
            });
        } catch {
            clearTimeout(timeout);
            showFallback();
        }
    };
})();
