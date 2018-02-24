tftp-server
===========

#### A [TFTP](https://tools.ietf.org/html/rfc1350) server implementation in Rust

Summary
----------
* Usable as both binary and library
* 100% safe code, no `unsafe` usage
* Well tested, including error cases
* Implements (some for now) RFCs describing extensions to the TFTP protocol

Building and running the server
-------------------------------

To build the server, simply run `cargo build`. Once compiled you can run the server without arguments, in which case it will pick a random port on the loopback address and serve from the current directory:

```
$ ./target/debug/tftp_server_bin
Server created at address: V4(127.0.0.1:61204)
```

In this example, the port number picked was 61204.

You can also explicitly specify the address (and optionally the port) on which it will listen

```
$ ./target/debug/tftp_server_bin --address 192.168.0.54
Server created at address: V4(192.168.0.54:43604)
```

or

```
$ ./target/debug/tftp_server_bin --address 192.168.0.54:35000
Server created at address: V4(192.168.0.54:35000)
```

If the server cannot bind to the given address:port (or if it cannot find a random port for the address) then it will panic with an IoError.
```
$ ./target/debug/tftp_server_bin --address 127.0.0.1:20
thread 'main' panicked at 'Error creating server: IoError(Error { repr: Os { code: 13, message: "Permission denied" } })', ../src/libcore/result.rs:799
note: Run with `RUST_BACKTRACE=1` for a backtrace.
```


Features
--------
All features are implemented in the library. The binary target is a only an argument-parsing thin wrapper over it for direct usage conveninence.

Available features:
* `-a` or `--address` to specify an address[:port] to listen on (multiple supported)
* `-r` will make the server treat the served directory as read-only (it will reject all write requests)
* `-d` or `--directory` specifies the directory to serve from (the given path will be prepended to all requested paths)
* `-t` or `--timeout` specifies the timeout (in seconds) for idle connections
* see TODO section below


TFTP Protocol Options & Extensions
---------------------
The following TFTP extension RFCs are implemented:
* [RFC 2347: TFTP Option Extension](https://tools.ietf.org/html/rfc2347)
* [RFC 2348: TFTP Blocksize Option](https://tools.ietf.org/html/rfc2348)
* (Not yet implemented) [RFC 2349: TFTP Timeout Interval and Transfer Size Options](https://tools.ietf.org/html/rfc2349)
* (Not yet implemented) [RFC 7440: TFTP Windowsize Option](https://tools.ietf.org/html/rfc7440)


Logging and Testing
-------------------

You can also run the server with logging enabled. To do this add `RUST_LOG=tftp_server=info` before the command.
For example:

```
$ RUST_LOG=tftp_server=info ./target/debug/tftp_server_bin
```

This will run the server with logging enabled so that you can inspect the program's behavior.

To run the tests you can just run `cargo test`. However if you want to show the program's output during the test,
you have to turn on logging. To run tests with logging enabled run:

```
$ RUST_LOG=tftp_server=info cargo test
```

TODOs
-----

* [ ] Documentation for individual items
* [ ] Crate-level documentation with an overview and examples
* [x] serve from specified directory, not just the current one
* [x] treat directory as readonly (reject write requests)
* [x] IPv6 support
* [x] multiple address support
* [ ] CLI switches for logging
* [ ] running control (ability to stop server hard or soft)
* [ ] limit accepted blocksize to stack MSS (smaller on ipv4)
* [ ] complete implementation of all option extension RFCs
* [ ] redo packets as in-place buffer references to avoid copying memory
* [ ] redo integration tests to run them with harness
* [ ] make proto tests more orthogonal
