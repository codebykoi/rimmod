# RimMod

RimWorld Mod Manager written in Rust.

## Workshop tab

The Workshop tab searches RimWorld Workshop items through Steam's Web API and
downloads or reinstalls them with SteamCMD. Both operations run in the background
so the application remains responsive.

To set it up, open **File > Settings** and configure:

- **Steam Web API key**: required for Workshop catalog searches. RimMod stores
  this key in its local application settings.
- **SteamCMD executable**: optional when `steamcmd` is already available on the
  system `PATH`.
- **Workshop location**: the `steamapps/workshop/content/294100` directory that
  RimMod scans for installed RimWorld Workshop items.

SteamCMD currently downloads with anonymous login. After a successful command,
RimMod scans the `steamapps/workshop/content/294100` folder beside SteamCMD
alongside the normal Steam Workshop folder. If both roots contain the same
Workshop item ID, the normal Steam folder takes priority.

Workshop cards use Steam's static thumbnails. Animated GIF previews are not
decoded or played, keeping catalog browsing responsive.
