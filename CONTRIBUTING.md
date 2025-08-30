## How to test docs.rs changes

Set up your local docs.rs environment as per official README:  
https://github.com/rust-lang/docs.rs?tab=readme-ov-file#getting-started

Make sure you have:
- Your .env contents exported to your local ENVs
- docker-compose stack for db and s3 running
- The web server running via local (or pure docker-compose approach)
- If on a remote machine, port 3000 (or whatever your webserver is listening on) forwarded

Invoke the cargo build command against your local path to your `tokio-metrics` workspace:

```
# you could also invoke the built `cratesfyi` binary from outside of your cargo workspace,
# though you'll still need the right ENVs exported
cargo run -- build crate --local ../tokio-metrics
```

Then, you can view the generated documentation for `tokio-metrics` in your browser. If you figure
out how to get CSS working, update this guide :)
