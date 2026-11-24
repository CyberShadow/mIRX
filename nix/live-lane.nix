# The live lane: the release mirx-ui.exe, driven through its GUI by the
# `live` test suite under Wine, against a real Conduit homeserver, all in a
# NixOS VM. scripts/live-lane.sh builds the Windows binaries on the host and
# passes them in as `bins` (a store path holding mirx-ui.exe and live.exe).
{ pkgs, wine, wineFonts, bins, filters, nonce }:
let
  inherit (pkgs) lib;
  # wine-runner.sh finds wine-prefix.sh through its parent's parent.
  scripts = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../scripts/wine-runner.sh
      ../scripts/wine-prefix.sh
    ];
  };
  # mirx-ui.exe as the tests' CreateProcessW must see it.
  mirxUiExe = "Z:" + lib.replaceStrings [ "/" ] [ "\\" ] "${bins}/mirx-ui.exe";
  homeserver = "http://127.0.0.1:6167";
  # The tests' switch for Conduit (conduit-switch below).
  switch = "127.0.0.1:6168";
  runner = lib.concatStringsSep " " (
    [
      "cd /tmp &&"
      "env"
      "MIRX_LIVE_HOMESERVER=${homeserver}"
      "MIRX_LIVE_SWITCH=${switch}"
      "MIRX_UI_EXE=${lib.escapeShellArg mirxUiExe}"
      "MIRX_WINE_FONTS=${wineFonts}"
      # Only here to make every run a new derivation: a gate must run.
      "MIRX_LIVE_NONCE=${lib.escapeShellArg nonce}"
      "TMPDIR=/tmp"
      "${scripts}/scripts/wine-runner.sh"
      "${bins}/live.exe"
      "--test-threads=1"
    ]
    ++ map lib.escapeShellArg filters
    # A command's background children must not hold the driver's pipes.
    ++ [ "</dev/null 2>&1" ]
  );
in
pkgs.testers.runNixOSTest {
  name = "mirx-live";
  globalTimeout = 3600;
  nodes.machine = {
    services.matrix-conduit = {
      enable = true;
      settings.global = {
        server_name = "localhost";
        address = "127.0.0.1";
        allow_registration = true;
        allow_federation = false;
        trusted_servers = [ ];
        # Clients follow /.well-known/matrix/client, which by default
        # sends them to https://<server_name>.
        well_known.client = homeserver;
      };
    };
    # A test stops and starts Conduit, as a server going down and coming
    # back: a line `stop` or `start` sent to the switch, which answers `ok`
    # once Conduit is stopped, or started and answering.
    systemd.sockets.conduit-switch = {
      wantedBy = [ "sockets.target" ];
      listenStreams = [ switch ];
      socketConfig.Accept = true;
    };
    systemd.services."conduit-switch@" = {
      path = [
        pkgs.systemd
        pkgs.curl
      ];
      serviceConfig = {
        StandardInput = "socket";
        StandardOutput = "socket";
        StandardError = "journal";
      };
      script = ''
        read -r action
        case "$action" in
          stop) systemctl stop conduit.service ;;
          start)
            systemctl start conduit.service
            until curl -sf ${homeserver}/_matrix/client/versions >/dev/null; do
              sleep 0.2
            done
            ;;
          *)
            echo "unknown action: $action" >&2
            exit 1
            ;;
        esac
        echo ok
      '';
    };
    # What wine-runner.sh requires (timeout and flock are in every
    # system), and curl for the test script.
    environment.systemPackages = [
      wine
      pkgs.xorg-server
      pkgs.xdpyinfo
      pkgs.imagemagick
      pkgs.curl
    ];
    virtualisation.memorySize = 6144;
    virtualisation.cores = 4;
    virtualisation.diskSize = 4096;
  };
  testScript = ''
    machine.wait_for_unit("conduit.service")
    machine.wait_for_open_port(6167)
    machine.wait_for_unit("conduit-switch.socket")
    # Conduit makes its first user an admin, in an admin room: that must not
    # be a test's user.
    machine.succeed(
        "curl -sf -X POST ${homeserver}/_matrix/client/v3/register"
        + " -H 'Content-Type: application/json'"
        + " -d '{\"username\":\"admin\",\"password\":\"admin-password\","
        + "\"auth\":{\"type\":\"m.login.dummy\"}}'"
    )
    # The driver's default timeout is 900 s.
    status, out = machine.execute(${builtins.toJSON runner}, timeout=2400)
    print(out)
    if status != 0:
        print(machine.execute("journalctl -u conduit --no-pager | tail -n 300")[1])
        raise Exception(f"the live tests failed (exit {status})")
  '';
}
