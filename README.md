Exclave: Factory Test Infrastructure
====================================

Exclave is a factory test infrastructure, written in Rust.  Tests are designed to be easy to write in any language you care to, as Exclave will capture the output from your program and log it all.  Once your test finishes, exit with a zero return code to indicate success, or nonzero to indicate failure.

Building
--------

To build, install [Rust](https:/rustup.rs).  To build the latest release, run:

    cargo install exclave

To build from source, check out this repository, change to the root directory, and run:

    cargo build

Running
-------

To run, you must specify a configuration directory with the "-c" argument.  For example, if your configuration directory is in /etc/exclave, run:

    exclave -c /etc/exclave

Or, if you're doing development and using cargo, run:

    cargo run -- -c /etc/exclave

If exclave detects that it's connected to a terminal, you will be presented with a live view of all units.  If it's not connected to a terminal (i.e. if it's running under systemd or init), then exclave will log all unit transitions to stdout, unless the "-q" option is specified.

Defining Configurations
-----------------------

An example configuration directory is present under "test/config".  You can use this to get started.  You can run it using "exclave -c test/config".

The unit configuration language is defined in doc/Units.md.

Writing Tests
-------------

Tests may be written in any language, but you must make sure the required interpreter (if any) is installed.

Log any progress to stdout, and log any error to stderr.

When a particular test has concluded, print the test result to stdout and exit.  If the test was successful, exit 0.  If the test failed, return nonzero.

Tests can time out, and if that occurs your test will first receive a SIGTERM.  After a configurable amount of time, your test will receive a SIGKILL.

All tests are run in their own session, and are connected to a pseudoterminal (PTY).  This will remove any buffering that would normally occur for things like printf.

Writing Interfaces, Loggers, and Triggers
-----------------------------------------

Interfaces, Loggers, and Triggers all must interact with exclave using custom streams.  The inter-process communication is documented in doc/IPC.md