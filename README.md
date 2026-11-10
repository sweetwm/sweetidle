<h1 align=center>sweetidle</h1>

<div align=center>

![GitHub last commit](https://img.shields.io/github/last-commit/naurissteins/sweetidle?style=for-the-badge&labelColor=181825&color=a6e3a1)
![GitHub repo size](https://img.shields.io/github/repo-size/naurissteins/sweetidle?style=for-the-badge&labelColor=181825&color=d3bfe6)
![AUR Version](https://img.shields.io/aur/version/sweetidle-bin?style=for-the-badge&labelColor=181825&color=b4befe)
![GitHub Repo stars](https://img.shields.io/github/stars/sweetwm/sweetidle?style=for-the-badge&labelColor=181825&color=f9e2af)

Lightweight and powerful idle daemon for Wayland compositors

</div>

## 🔥 Features
- Multiple idle listeners, each with its own timeout and resume command
- Lock before suspend and run hooks after resume
- Handle logind lock and unlock requests on Linux
- Respect compositor and D-Bus idle inhibitors, plus systemd inhibitors on Linux
- Hot reload with no restart (a bad config keeps the old one running)
- Control commands: `status`, `pause`, `resume`, and `reload`
- Simple TOML config that is checked before it is used

## Install

### Arch Linux

Install sweetidle from the AUR:

```bash
# prebuilt release package
yay -S sweetidle-bin

# git
yay -S sweetidle-git
```

Copy the example config, then start the user service:

```bash
mkdir -p ~/.config/sweetidle
curl -o ~/.config/sweetidle/config.toml \
  https://raw.githubusercontent.com/naurissteins/sweetidle/main/config/example.toml
systemctl --user enable --now sweetidle.service
```

### NixOS

Add the flake to your inputs and import the module:

```nix
{
  inputs = {
    nixpkgs.url = "nixpkgs/nixos-26.05";
    sweetidle = {
      url = "github:naurissteins/sweetidle";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = {nixpkgs, sweetidle, ...}: {
    nixosConfigurations.myhost = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        ./configuration.nix
        sweetidle.nixosModules.default
      ];
    };
  };
}
```

Enable the service:

```nix
services.sweetidle = {
  enable = true;
  settings = {
    general.before_sleep_command = "veila lock --wait-ready";
    listener = [
      { timeout = 300; on_timeout = "veila lock --wait-ready"; }
      {
        timeout = 600;
        on_timeout = "sweets msg output-power '*' off";
        on_resume = "sweets msg output-power '*' on";
      }
    ];
  };
};
```

Leave `settings` empty to use `~/.config/sweetidle/config.toml` instead.

### FreeBSD

Tested on FreeBSD 15.1 amd64 with Sweets. Build from source and start it from
your compositor. See the [FreeBSD guide](https://sweetwm.org/docs/sweetidle/freebsd)
for setup, sleep hooks, and limits.

## Config

Config lives in `~/.config/sweetidle/config.toml`. Check it with:

```bash
sweetidle --check-config
```

### Lock after 5 minutes

```toml
[[listener]]
timeout = 300
on_timeout = "veila lock --wait-ready"
ignore_inhibit = true # lock even while a video plays
```

### Turn off displays after 10 minutes

```toml
[[listener]]
timeout = 600
on_timeout = "sweets msg output-power '*' off"
on_resume = "sweets msg output-power '*' on"
```

### Lock before suspend

Requires Linux with logind, or the [FreeBSD rc hook](https://sweetwm.org/docs/sweetidle/freebsd#sleep-hooks).

```toml
[general]
before_sleep_command = "veila lock --wait-ready"
after_resume_command = "sweets msg output-power '*' on"
wait_before_sleep = true
```

### Handle `loginctl lock-session`

Requires Linux with logind.

```toml
[general]
lock_command = "veila lock --wait-ready"
```

### Ignore app inhibitors

```toml
[general]
ignore_dbus_inhibit = true
ignore_systemd_inhibit = true
```

See the [Linux example](config/example.toml) or [FreeBSD example](config/freebsd.toml).

## Usage

```bash
sweetidle status    # show daemon state
sweetidle pause     # stop idle timers
sweetidle resume    # start timers again
sweetidle reload    # reload config
```

`status` shows whether systemd inhibitor monitoring is `enabled`, `ignored`, or
`unsupported`. `status --json` includes this as `inhibitors.systemd_status`;
the existing version 1 fields keep their meanings.

Logs for the Linux systemd user service:

```bash
journalctl --user -u sweetidle.service -f
```
