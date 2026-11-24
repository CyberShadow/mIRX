mirx
====

mirx is a lightweight native Windows client for Matrix, the open chat
network: rooms, direct messages, end-to-end encryption, in a classic
multi-window chat client with a treebar, a switchbar and slash commands.

It is early software, built straight from its source repository by its
CI. Expect rough edges, and keep a second Matrix client at hand.

Running it
----------

- Windows 10 or 11, 64-bit.
- Extract the folder anywhere and run mirx-ui.exe. Nothing is installed.
- The exe is not signed: Windows SmartScreen may say "Windows protected
  your PC". Choose "More info", then "Run anyway".
- On exit the window closes at once; mirx-ui.exe may take up to a few
  seconds more to end, while it tells the server what you read.

Signing in
----------

The first start opens Select Server. Type your server (for example
matrix.org) and your user name and password. Leave Password empty to sign
in through your browser instead, as servers using the Matrix
Authentication Service (matrix.org among them) and single sign-on need.

mirx keeps the session: the next start connects by itself. File > Log Out
ends the session and removes its messages, room keys and sync state from
this computer. With encryption, set up recovery first (/e2ee setup) or
export your room keys (Tools > Export Room Keys...), or a new session cannot
read the encrypted history.

Your data
---------

Settings (mirx.ini), the session, logs and crash reports live in
%LOCALAPPDATA%\mirx\ui-shell. To remove mirx, log out, delete this folder
and that one.

Licences
--------

mirx is licensed under the Apache License 2.0 (LICENSE.txt).
THIRD-PARTY-LICENSES.txt lists the software it is built from and their
licences; COPYING.MinGW-w64-runtime.txt has the MinGW-w64 runtime's.

Source: https://github.com/CyberShadow/mIRX
