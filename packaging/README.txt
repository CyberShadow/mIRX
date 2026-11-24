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

If your server cannot be reached (or answers with an error) when you log
out, Log Out offers to remove the session from this computer only. Your
server then still lists the session until you sign it out from another
one: Tools > Sessions... in mirx there, or the session list of another
Matrix app.

Verifying
---------

A new session reads the encrypted history once it is verified: Tools >
Verify This Session asks your other sessions. Accept there, compare the
emoji Status shows with theirs, then Tools > Confirm SAS (or /verify
confirm). Several verifications can be under way at once, with different
people: Status names each by user and session, the Tools items and the
/verify answers without a name act on the most recently started one, and
/verify confirm <user> [session] answers a given one.

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
