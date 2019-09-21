# fake-irc-server

A fake IRC Server. Originally written to debug a plugin for WeeChat IRC client.

# Compiling

You need rustc to compile it.

```
$ rustc fake-irc-server.rs
```

# Running

Pass port number in the first argument. Default port is 1234.

```
$ ./fake-irc-server 1234
```

Now you can connect to the localhost, port 1234, from your IRC client. For example, on WeeChat, execute:

```
/connect localhost:1234
```

# LICENSE

MIT LICENSE
